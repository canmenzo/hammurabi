use std::io::{BufRead, BufReader};
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Context;
use regex::Regex;
use serde_json::{Value, json};

use crate::settings::lang_name;
use crate::types::Translate;

const USER_AGENT: &str = "OpenAI File Downloader, XaiImageApiFetch/1.0";
/// The longest caption translates to about half of this; more is a model that will not stop.
const MAX_TOKENS: u32 = 64;
const HINT_CHARS: usize = 600;
/// For targets in the Latin alphabet, our own server may only write Latin letters, punctuation
/// and symbols. Once in thousands of lines the model ends a word in East Asian punctuation.
const LATIN_GRAMMAR: &str = r"root ::= [\x20-ɏ‐-⃏]+";

/// OpenAI-compatible chat endpoint: the user's own (Ollama `http://localhost:11434/v1`) or the
/// llama.cpp server Hammurabi runs itself.
pub struct LlmTranslator {
    base_url: String,
    model: String,
    agent: ureq::Agent,
    /// The server is our own llama.cpp: it does not think aloud, and answers within a second.
    own: bool,
    /// Names and words to keep as they are (`Settings::hint`).
    hint: String,
}

/// `timeout` for each step of a request, and none for looking the host up (the system has its
/// own). With one for the request as a whole, the client starts a thread to look the host up for
/// every connection it makes, and NVIDIA's OpenGL driver keeps memory for every thread a process
/// with a window ever had: half a gigabyte for an hour of captions.
fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(timeout))
        .timeout_send_request(Some(timeout))
        .timeout_send_body(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_recv_body(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

impl LlmTranslator {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            agent: agent(Duration::from_secs(10)),
            own: false,
            hint: String::new(),
        }
    }

    /// For the llama.cpp server Hammurabi started. A line that takes seconds there means the
    /// graphics memory is full; the caption goes out untranslated instead of that late.
    pub fn own(base_url: &str) -> Self {
        Self {
            agent: agent(Duration::from_secs(3)),
            own: true,
            ..Self::new(base_url, "")
        }
    }

    /// Runs the model once, however long that takes: the first lines through a freshly started
    /// server take seconds (it builds its GPU programs), which captions must not wait for.
    pub fn warm_up(&self) -> anyhow::Result<()> {
        let mut patient = Self {
            agent: agent(Duration::from_secs(120)),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            own: self.own,
            hint: String::new(),
        };
        for line in [
            "Hey.",
            "We have to lay low for a while, they are looking for us everywhere.",
        ] {
            patient.translate(line, "en", "tr", &[])?;
        }
        Ok(())
    }

    /// Checks the endpoint is reachable.
    pub fn ping(&self) -> anyhow::Result<()> {
        let url = &self.base_url;
        let mut resp = agent(Duration::from_secs(2))
            .get(&format!("{url}/models"))
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| {
                log::warn!("LLM ping {url}: {e}");
                anyhow::anyhow!(
                    "can't reach the local LLM at {url}, is Ollama or LM Studio running?"
                )
            })?;
        anyhow::ensure!(
            resp.status().is_success(),
            "the local LLM at {url} returned {}",
            resp.status()
        );
        // OpenAI-style `{"data": [{"id": ...}]}`; servers that list nothing are trusted.
        let models: Vec<String> = resp
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|b| serde_json::from_str::<Value>(&b).ok())
            .and_then(|v| v["data"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect();
        anyhow::ensure!(
            models.is_empty()
                || models
                    .iter()
                    .any(|m| *m == self.model || *m == format!("{}:latest", self.model)),
            "the local LLM has no model named {} (it has: {})",
            self.model,
            models.join(", ")
        );
        Ok(())
    }
}

/// What the model is told. The wording, the Turkish notes and the examples were tuned on game
/// dialogue (street slang, swearing, speech recognition's mistakes) and scored by hand.
fn system(src: &str, tgt: &str, hint: &str) -> String {
    // A list far longer than the names of one game or film would crowd out the lines themselves.
    let hint: String = hint.trim().chars().take(HINT_CHARS).collect();
    let names = if hint.is_empty() {
        String::new()
    } else {
        format!(": {}", hint.trim_end_matches('.'))
    };
    let notes = if (src, tgt) == ("English", "Turkish") {
        TURKISH
    } else {
        "- Keep profanity as strong as it is. A slur used between friends as a form of address \
         becomes a friendly form of address, never a racial word.\n"
    };
    format!(
        "You translate live {src} subtitles of films, games and streams into {tgt}, like a skilled \
         {tgt} fansub team. Lines come from speech recognition, so they may lack punctuation, be \
         cut off mid-sentence or contain misheard words; translate what was most likely said.\n\n\
         Rules:\n\
         - Reply with only the {tgt} subtitle for the last user line, with normal {tgt} \
         capitalization and punctuation. No quotes, notes, explanations or {src}.\n\
         - Never answer, comment on or refuse the line. Always translate it, however rude.\n\
         - Translate the meaning, not the words: slang and idioms get natural spoken {tgt} \
         equivalents. Keep it short.\n\
         {notes}\
         - Keep names and places unchanged{names}.\n\
         - Earlier lines are context only."
    )
}

const TURKISH: &str = "\
- Address words like homie, dog, dawg, bro, cuz and the N-word become kanka, moruk, birader or lan \
(never köpek). Never render the N-word as a racial word. Where the word itself is what is talked \
about (who you calling a nigga, don't say nigga), it is \"o kelime\": kime o kelimeyi diyorsun lan.
- Keep profanity at natural Turkish strength: fuck = siktir / amına koyayım, motherfucker = orospu \
çocuğu, bitch = kaltak / şerefsiz, asshole = pislik / göt herif, shit = siktir / bok. Avoid tame \
words like \"lanet olası\". \"The fuck\" as an intensifier (calm the fuck down, what the fuck) is \
lan or amına koyayım, not siktir git.
- Slang: finna = -ecek; ain't = değil/yok; trippin' = kafayı yemek; whip = araba; plug = bağlantı; \
snitch = ötmek; smoked = öldürüldü; cappin' = atmak (yalan); keep it a hunnid = dürüst ol; bet = \
tamamdır; real talk = ciddiyim; opps = düşmanlar; OG = abi; ho = kaşar; forty = bira; lit up / \
light up = kurşun yağdırmak; stoner = esrarkeş; damn = vay be / lan; bad bitch = afet (a \
compliment); pony = at kuyruğu; you want some? = dayak mı istiyorsun.
";

/// Example lines and their translations, shown to the model as earlier turns.
fn shots(src: &str, tgt: &str) -> &'static [(&'static str, &'static str)] {
    if (src, tgt) != ("English", "Turkish") {
        return &[];
    }
    &[
        (
            "Nigga, you crazy as hell.",
            "Kanka, sen kafayı sıyırmışsın.",
        ),
        (
            "ay fool quit playin and get in the car",
            "Lan, oyalanmayı bırak da bin arabaya.",
        ),
        (
            "Man, fuck these guys, they shot my homie.",
            "Bu heriflerin amına koyayım, kankamı vurdular.",
        ),
        (
            "We gotta lay low for a minute, dog.",
            "Bir süre ortalıktan kaybolmamız lazım moruk.",
        ),
        ("Shit, put the gun down!", "Siktir, indir şu silahı!"),
    ]
}

impl Translate for LlmTranslator {
    fn set_hint(&mut self, hint: &str) {
        if hint != self.hint {
            self.hint = hint.to_string();
        }
    }

    fn translate(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
    ) -> anyhow::Result<String> {
        self.translate_unless(text, source, target, context, &|| false)?
            .context("translation given up")
    }

    fn translate_unless(
        &mut self,
        text: &str,
        source: &str,
        target: &str,
        context: &[(String, String)],
        stop: &dyn Fn() -> bool,
    ) -> anyhow::Result<Option<String>> {
        let (src, tgt) = (lang_name(source), lang_name(target));
        let mut messages = vec![json!({"role": "system", "content": system(src, tgt, &self.hint)})];
        let shots = shots(src, tgt).iter().copied();
        let context = context.iter().map(|(o, t)| (o.as_str(), t.as_str()));
        for (orig, trans) in shots.chain(context) {
            messages.push(json!({"role": "user", "content": orig}));
            messages.push(json!({"role": "assistant", "content": trans}));
        }
        messages.push(json!({"role": "user", "content": text}));

        // Greedy: the same line always gets the same translation.
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            // Our own server is read as it writes, so that it can be cut short.
            "stream": self.own,
            "temperature": 0,
            "max_tokens": MAX_TOKENS,
        });
        if self.own {
            // A caption is one line, and the unchanging start of the prompt is not read again.
            // Not for other servers: their models may think aloud first, over several lines.
            body["stop"] = json!(["\n"]);
            body["cache_prompt"] = json!(true);
            if LATIN.contains(&target) {
                body["grammar"] = json!(LATIN_GRAMMAR);
            }
        }
        let mut resp = self
            .agent
            .post(&format!("{}/chat/completions", self.base_url))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .context("LLM request failed")?;
        let status = resp.status();
        let content = if self.own && status.is_success() {
            let mut content = String::new();
            for line in BufReader::new(resp.body_mut().as_reader()).lines() {
                let line = line.context("LLM response unreadable")?;
                // Leaving closes the connection, at which the server stops writing.
                if stop() {
                    return Ok(None);
                }
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    break;
                }
                let v: Value = serde_json::from_str(data).context("LLM response is not JSON")?;
                if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
                    content.push_str(piece);
                }
            }
            content
        } else {
            let raw = resp
                .body_mut()
                .read_to_string()
                .context("LLM response unreadable")?;
            anyhow::ensure!(status.is_success(), "LLM returned {status}: {raw}");
            let v: Value = serde_json::from_str(&raw).context("LLM response is not JSON")?;
            v["choices"][0]["message"]["content"]
                .as_str()
                .context("LLM response has no content")?
                .to_string()
        };
        let out = clean(&content);
        check(text, &out, target)?;
        Ok(Some(scrub(&out, target)))
    }
}

/// The model keeps the N-word as it is when it is talked about rather than used ("you can't say
/// ..."), and in Turkish it may name it ("zenci kelimesi") or fall back on "zenci" for a person.
/// A stream cannot show the first, and "siyahi" is the word for the last that offends no one.
fn scrub(out: &str, target: &str) -> String {
    static SLUR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bnigg(?:a|er)s?\b").unwrap());
    static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bzenci kelimesi").unwrap());
    static ZENCI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bzenci").unwrap());
    // "That word", capitalized where what it replaces was.
    let that = |c: &regex::Captures| {
        let upper = c[0].starts_with(char::is_uppercase);
        if upper { "O kelime" } else { "o kelime" }.to_string()
    };
    let out = SLUR.replace_all(out, that);
    if target != "tr" {
        return out.into_owned();
    }
    let out = NAMED.replace_all(&out, that);
    ZENCI
        .replace_all(&out, |c: &regex::Captures| {
            let upper = c[0].starts_with(char::is_uppercase);
            if upper { "Siyahi" } else { "siyahi" }.to_string()
        })
        .into_owned()
}

/// Languages written in the Latin alphabet, among the translation targets.
const LATIN: [&str; 15] = [
    "tr", "en", "es", "fr", "de", "it", "pt", "id", "vi", "nl", "pl", "sv", "ro", "cs", "hu",
];

/// Rejects what cannot be a translation of `text`: nothing, far more than was said, or (seen once
/// in thousands of lines, with the graphics memory full) characters of another script.
fn check(text: &str, out: &str, target: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!out.is_empty(), "the LLM answered nothing");
    let (said, got) = (text.chars().count(), out.chars().count());
    anyhow::ensure!(
        got <= 3 * said + 40,
        "the LLM answered more than was said: {out}"
    );
    // Latin Extended ends at U+024F. Past it, letters and East Asian punctuation are foreign;
    // dashes, quotes and the like are not.
    let foreign = |c: char| {
        c > '\u{24f}'
            && (c.is_alphabetic() || matches!(c, '\u{3000}'..='\u{303f}' | '\u{ff00}'..='\u{ffef}'))
    };
    anyhow::ensure!(
        !LATIN.contains(&target) || !out.chars().any(foreign),
        "the LLM answered in another script: {out}"
    );
    Ok(())
}

/// The answer itself: without what a model thought first, on its first line, without quotes.
fn clean(s: &str) -> String {
    let mut s = s.to_string();
    while let Some(start) = s.find("<think>") {
        match s[start..].find("</think>") {
            Some(end) => s.replace_range(start..start + end + "</think>".len(), ""),
            None => {
                s.truncate(start);
                break;
            }
        }
    }
    if let Some(end) = s.rfind("</think>") {
        s.drain(..end + "</think>".len());
    }
    let line = s.trim().lines().next().unwrap_or_default().trim();
    // Quotes around the whole line go; ones that open or close a part of it stay.
    let quotes = [('"', '"'), ('\'', '\''), ('“', '”'), ('‘', '’'), ('«', '»')];
    let inner = quotes.iter().find_map(|&(open, close)| {
        let inner = line.strip_prefix(open)?.strip_suffix(close)?;
        (!inner.contains([open, close])).then_some(inner)
    });
    inner.unwrap_or(line).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_and_quotes() {
        assert_eq!(
            clean("<think>\nhmm\n</think>\n\n\"Merhaba dünya\"\n"),
            "Merhaba dünya"
        );
        assert_eq!(clean("  “Hola”  "), "Hola");
        assert_eq!(clean("reasoning</think>Hi"), "Hi");
        assert_eq!(clean("plain"), "plain");
        assert_eq!(clean("Selam.\n(Note: informal)"), "Selam.");
        assert_eq!(
            clean("\"Ben kaçtım\" dedi ve gitti."),
            "\"Ben kaçtım\" dedi ve gitti."
        );
    }

    #[test]
    fn words_a_stream_cannot_show_are_replaced() {
        assert_eq!(
            scrub("Yayında nigga diyemezsin.", "tr"),
            "Yayında o kelime diyemezsin."
        );
        assert_eq!(
            scrub("Zenci kelimesi. Bu hiç hoş değil, kanka.", "tr"),
            "O kelime. Bu hiç hoş değil, kanka."
        );
        assert_eq!(scrub("İki zenciyi gördüm.", "tr"), "İki siyahiyi gördüm.");
        assert_eq!(scrub("Niggas, bitte.", "de"), "O kelime, bitte.");
        assert_eq!(scrub("Kanka, naber?", "tr"), "Kanka, naber?");
    }

    #[test]
    fn rejects_what_is_no_translation() {
        assert!(check("Hello there.", "Selam, n'aber? Çok iyi…", "tr").is_ok());
        assert!(check("Hello there.", "", "tr").is_err());
        assert!(
            check(
                "These fools will get us killed.",
                "Bu aptallar bizi öld》。",
                "tr"
            )
            .is_err()
        );
        assert!(check("These fools.", "Bu aptallar 我们", "tr").is_err());
        assert!(check("Hello.", "こんにちは。", "ja").is_ok());
        assert!(check("Hi.", &"çok ".repeat(30), "tr").is_err());
    }

    #[test]
    fn prompt_names_the_languages_and_the_words_to_keep() {
        let p = system("English", "Turkish", "Lamar, Vice City.");
        assert!(p.contains("into Turkish") && p.contains("kanka"));
        assert!(p.contains("unchanged: Lamar, Vice City.\n"));
        assert_eq!(shots("English", "Turkish").len(), 5);
        let p = system("Japanese", "German", " ");
        assert!(p.contains("live Japanese subtitles") && !p.contains("kanka"));
        assert!(p.contains("unchanged.\n") && p.contains("never a racial word"));
        assert!(shots("Japanese", "German").is_empty());
    }

    #[test]
    #[ignore = "needs local Ollama with a model, set HAMMURABI_LLM_MODEL"]
    fn ollama_translates() {
        let model = std::env::var("HAMMURABI_LLM_MODEL").unwrap_or_else(|_| "qwen2.5:1.5b".into());
        let mut t = LlmTranslator::new("http://localhost:11434/v1", &model);
        t.ping().unwrap();
        let out = t
            .translate("Good morning, how are you?", "en", "tr", &[])
            .unwrap();
        println!("{out}");
        assert!(!out.is_empty());
    }
}
