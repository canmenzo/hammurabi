use serde::{Deserialize, Serialize};

use crate::tts::{self, Style};
use crate::types::AudioSource;

/// Languages Whisper is offered for (Babil's list).
pub const SOURCE_LANGS: [&str; 28] = [
    "en", "es", "fr", "de", "it", "pt", "nl", "pl", "ru", "uk", "tr", "ar", "fa", "hi", "bn", "ur",
    "id", "vi", "th", "ja", "ko", "zh", "el", "sv", "ro", "cs", "hu", "he",
];

/// Translation targets, most spoken first (Babil's list).
pub const TARGET_LANGS: [&str; 26] = [
    "tr", "en", "es", "fr", "de", "it", "pt", "ru", "ar", "hi", "bn", "zh", "zh-Hant", "ja", "ko",
    "id", "vi", "th", "nl", "pl", "uk", "el", "sv", "ro", "cs", "hu",
];

/// English display name; unknown codes are returned as-is.
pub fn lang_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "pl" => "Polish",
        "ru" => "Russian",
        "uk" => "Ukrainian",
        "tr" => "Turkish",
        "ar" => "Arabic",
        "fa" => "Persian",
        "hi" => "Hindi",
        "bn" => "Bengali",
        "ur" => "Urdu",
        "id" => "Indonesian",
        "vi" => "Vietnamese",
        "th" => "Thai",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        "zh-Hant" => "Chinese (Traditional)",
        "el" => "Greek",
        "sv" => "Swedish",
        "ro" => "Romanian",
        "cs" => "Czech",
        "hu" => "Hungarian",
        "he" => "Hebrew",
        _ => code,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsrBackend {
    Whisper,
    WindowsLiveCaptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelChoice {
    Auto,
    Tiny,
    Base,
    Small,
    Turbo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranslatorChoice {
    Builtin,
    Llm { base_url: String, model: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    None,
    Shadow,
    Outline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionStyle {
    pub trans_size: f32,
    pub orig_size: f32,
    pub trans_color: String,
    pub orig_color: String,
    pub bg_opacity: f32,
    pub show_original: bool,
    pub original_first: bool,
    pub cues: u32,
    pub hold_secs: f32,
    pub edge: Edge,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            trans_size: 30.0,
            orig_size: 20.0,
            trans_color: "#ffffff".into(),
            orig_color: "#ffe066".into(),
            bg_opacity: 0.75,
            show_original: true,
            original_first: false,
            cues: 2,
            hold_secs: 4.0,
            edge: Edge::Shadow,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub source: AudioSource,
    /// cpal device id; None = OS default.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    /// None = auto-detect.
    pub spoken_language: Option<String>,
    pub asr_backend: AsrBackend,
    pub model: ModelChoice,
    /// Encode only the speech plus a margin instead of Whisper's full 30 s window (much faster on CPU).
    pub trim_audio: bool,
    pub translate: bool,
    pub target_language: String,
    pub translator: TranslatorChoice,
    pub instant: bool,
    /// Read final captions aloud (the translation when there is one) with an OS voice.
    pub speak: bool,
    /// Id from `tts::PERSONAS`. Empty only while loading, so `migrated` can tell it was missing.
    #[serde(default)]
    pub speak_persona: String,
    /// Voice id from `tts::voices`, used for captions in its language; None = the OS default.
    /// Custom persona only.
    pub speak_voice: Option<String>,
    /// 1.0 = normal speed. Custom persona only.
    pub speak_rate: f32,
    pub speak_engine: tts::Engine,
    pub caption: CaptionStyle,
    /// Logical px.
    pub overlay_pos: Option<[f32; 2]>,
    /// Parsed with `str::parse::<global_hotkey::hotkey::HotKey>()`.
    pub hotkey_toggle: String,
    pub hotkey_move: String,
    /// Ask GitHub for a newer release at startup and once a day.
    pub check_updates: bool,
    /// Last version the overlay announced, so each update is announced once.
    pub update_notified: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source: AudioSource::System,
            input_device: None,
            output_device: None,
            spoken_language: None,
            asr_backend: AsrBackend::Whisper,
            model: ModelChoice::Auto,
            trim_audio: true,
            translate: true,
            target_language: default_target(sys_locale::get_locale().as_deref()),
            translator: TranslatorChoice::Builtin,
            instant: false,
            speak: false,
            speak_persona: tts::PERSONAS[0].id.into(),
            speak_voice: None,
            speak_rate: 1.0,
            speak_engine: tts::Engine::System,
            caption: CaptionStyle::default(),
            overlay_pos: None,
            hotkey_toggle: "alt+shift+KeyT".into(),
            hotkey_move: "alt+shift+KeyM".into(),
            check_updates: true,
            update_notified: None,
        }
    }
}

impl Settings {
    /// Configs from before personas keep the voice and speed picked there, as Custom.
    pub fn migrated(mut self) -> Self {
        if self.speak_persona.is_empty() {
            let picked = self.speak_voice.as_deref().is_some_and(|v| !v.is_empty())
                || self.speak_rate != 1.0;
            self.speak_persona = if picked {
                tts::CUSTOM
            } else {
                tts::PERSONAS[0].id
            }
            .into();
        }
        self
    }

    pub fn speak_style(&self) -> Style {
        Style {
            persona: self.speak_persona.clone(),
            voice: self.speak_voice.clone(),
            rate: self.speak_rate,
            engine: self.speak_engine,
        }
    }

    /// The language captions are read aloud in, when known: the translation's, else the spoken one.
    pub fn speak_lang(&self) -> Option<&str> {
        if self.translate {
            Some(self.target_language.as_str())
        } else {
            self.spoken_language.as_deref()
        }
    }
}

/// Maps a BCP 47 locale ("tr-TR", "zh-Hant-TW", "zh_TW") to a TARGET_LANGS code, else "en".
fn default_target(locale: Option<&str>) -> String {
    let Some(locale) = locale else {
        return "en".into();
    };
    let parts: Vec<String> = locale
        .split(['-', '_'])
        .map(str::to_ascii_lowercase)
        .collect();
    let code = match parts[0].as_str() {
        "zh" if parts[1..]
            .iter()
            .any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo")) =>
        {
            "zh-Hant"
        }
        p => p,
    };
    TARGET_LANGS
        .iter()
        .find(|&&c| c == code)
        .map_or("en", |c| c)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_from_locale() {
        assert_eq!(default_target(Some("tr-TR")), "tr");
        assert_eq!(default_target(Some("zh-Hant-TW")), "zh-Hant");
        assert_eq!(default_target(Some("zh_TW")), "zh-Hant");
        assert_eq!(default_target(Some("zh-CN")), "zh");
        assert_eq!(default_target(Some("sw-KE")), "en");
        assert_eq!(default_target(None), "en");
    }

    #[test]
    fn every_lang_has_a_name() {
        for c in SOURCE_LANGS.iter().chain(TARGET_LANGS.iter()) {
            assert_ne!(lang_name(c), *c);
        }
    }

    #[test]
    fn toml_round_trip() {
        let s = Settings {
            translator: TranslatorChoice::Llm {
                base_url: "http://localhost:11434/v1".into(),
                model: "qwen3".into(),
            },
            overlay_pos: Some([10.0, 20.0]),
            speak_engine: tts::Engine::Supertonic,
            ..Settings::default()
        };
        let text = toml::to_string(&s).unwrap();
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        assert_eq!(
            toml::from_str::<Settings>("").unwrap().migrated(),
            Settings::default()
        );
    }

    #[test]
    fn persona_migration() {
        let persona = |toml: &str| {
            toml::from_str::<Settings>(toml)
                .unwrap()
                .migrated()
                .speak_persona
        };
        assert_eq!(persona(""), "lale");
        assert_eq!(persona("speak = true"), "lale");
        assert_eq!(persona("speak_voice = \"\""), "lale");
        assert_eq!(persona("speak_rate = 1.0"), "lale");
        assert_eq!(persona("speak_voice = \"tr-1\""), "custom");
        assert_eq!(persona("speak_rate = 1.3"), "custom");
        assert_eq!(
            persona("speak_persona = \"ayla\"\nspeak_voice = \"tr-1\""),
            "ayla"
        );
        let s = toml::from_str::<Settings>("speak_voice = \"tr-1\"\nspeak_rate = 1.3")
            .unwrap()
            .migrated();
        assert_eq!(
            s.speak_style(),
            Style {
                persona: "custom".into(),
                voice: Some("tr-1".into()),
                rate: 1.3,
                engine: tts::Engine::System,
            }
        );
    }
}
