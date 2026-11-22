//! The app's own words in English, Turkish, Spanish, Brazilian Portuguese and Simplified Chinese.
//! `t` looks a text up in the language set last with `set` (English until then, as in tests and
//! the CLI). Texts take arguments as `{0}`, `{1}`... through `tf`.

use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "tr")]
    Tr,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "pt-BR")]
    Pt,
    #[serde(rename = "zh-Hans")]
    Zh,
}

pub const LANGS: [Lang; 5] = [Lang::En, Lang::Tr, Lang::Es, Lang::Pt, Lang::Zh];

impl Lang {
    /// The language's name in itself, as the picker shows it.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Tr => "Türkçe",
            Lang::Es => "Español",
            Lang::Pt => "Português (Brasil)",
            Lang::Zh => "简体中文",
        }
    }

    /// Writes a decimal comma rather than a point.
    fn comma(self) -> bool {
        matches!(self, Lang::Tr | Lang::Es | Lang::Pt)
    }
}

/// The UI language for a BCP 47 locale ("tr-TR", "pt_BR", "zh-Hans-CN"); English when it is
/// none of the five. Traditional Chinese is English too, as these texts are Simplified.
pub fn from_locale(locale: &str) -> Lang {
    let parts: Vec<String> = locale
        .split(['-', '_', '.', '@'])
        .map(str::to_ascii_lowercase)
        .collect();
    match parts[0].as_str() {
        "tr" => Lang::Tr,
        "es" => Lang::Es,
        "pt" => Lang::Pt,
        "zh" if !parts[1..]
            .iter()
            .any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo")) =>
        {
            Lang::Zh
        }
        _ => Lang::En,
    }
}

/// The OS's UI language, asked once.
pub fn system() -> Lang {
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(|| sys_locale::get_locale().map_or(Lang::En, |l| from_locale(&l)))
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Uses `choice`, or the system's language for None.
pub fn set(choice: Option<Lang>) {
    let lang = choice.unwrap_or_else(system);
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    LANGS[CURRENT.load(Ordering::Relaxed) as usize]
}

/// `key` in the current language, English where it has none.
pub fn t(key: T) -> &'static str {
    text_in(key, lang())
}

/// `key` in English, for the log.
pub fn en(key: T) -> &'static str {
    key.texts()[0]
}

fn text_in(key: T, lang: Lang) -> &'static str {
    let texts = key.texts();
    match texts[lang as usize] {
        "" => texts[0],
        s => s,
    }
}

/// `key` with `{0}`, `{1}`... replaced by `args`.
pub fn tf(key: T, args: &[&dyn Display]) -> String {
    let mut s = t(key).to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

/// The language's name in the app's language; unknown codes are returned as-is.
pub fn language_name(code: &str) -> &str {
    let key = match code {
        "en" => T::LangEn,
        "es" => T::LangEs,
        "fr" => T::LangFr,
        "de" => T::LangDe,
        "it" => T::LangIt,
        "pt" => T::LangPt,
        "nl" => T::LangNl,
        "pl" => T::LangPl,
        "ru" => T::LangRu,
        "uk" => T::LangUk,
        "tr" => T::LangTr,
        "ar" => T::LangAr,
        "fa" => T::LangFa,
        "hi" => T::LangHi,
        "bn" => T::LangBn,
        "ur" => T::LangUr,
        "id" => T::LangId,
        "vi" => T::LangVi,
        "th" => T::LangTh,
        "ja" => T::LangJa,
        "ko" => T::LangKo,
        "zh" => T::LangZh,
        "zh-Hant" => T::LangZhHant,
        "el" => T::LangEl,
        "sv" => T::LangSv,
        "ro" => T::LangRo,
        "cs" => T::LangCs,
        "hu" => T::LangHu,
        "he" => T::LangHe,
        _ => return code,
    };
    t(key)
}

/// `x` with `places` decimals, and the decimal comma where the language writes one.
pub fn decimal(x: f64, places: usize) -> String {
    let s = format!("{x:.places$}");
    if lang().comma() {
        s.replace('.', ",")
    } else {
        s
    }
}

macro_rules! texts {
    ($($key:ident => [$en:expr, $tr:expr, $es:expr, $pt:expr, $zh:expr $(,)?],)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum T {
            $($key,)*
        }

        impl T {
            #[cfg(test)]
            const ALL: &[T] = &[$(T::$key,)*];

            /// English, Turkish, Spanish, Portuguese, Chinese: the order of `Lang`.
            fn texts(self) -> [&'static str; 5] {
                match self {
                    $(T::$key => [$en, $tr, $es, $pt, $zh],)*
                }
            }
        }
    };
}

texts! {
    // Settings pages
    PageListen => ["Listen", "Dinleme", "Escuchar", "Ouvir", "收听"],
    PageTranslate => ["Translate", "Çeviri", "Traducir", "Traduzir", "翻译"],
    PageReadAloud => ["Read aloud", "Sesli okuma", "Leer en voz alta", "Ler em voz alta", "朗读"],
    PageCaptions => ["Captions", "Altyazılar", "Subtítulos", "Legendas", "字幕"],
    PageGeneral => ["General", "Genel", "General", "Geral", "常规"],
    SettingsHeading => ["Settings", "Ayarlar", "Ajustes", "Configurações", "设置"],

    // Header
    CaptionsOff => ["Captions are off", "Altyazılar kapalı", "Los subtítulos están desactivados", "As legendas estão desligadas", "字幕已关闭"],
    GettingReady => ["Getting ready\u{2026}", "Hazırlanıyor\u{2026}", "Preparando\u{2026}", "Preparando\u{2026}", "正在准备\u{2026}"],
    CaptionsOn => ["Captions are on", "Altyazılar açık", "Los subtítulos están activados", "As legendas estão ligadas", "字幕已开启"],
    StartCaptions => ["Start captions", "Altyazıları başlat", "Iniciar subtítulos", "Iniciar legendas", "开始字幕"],
    StopCaptions => ["Stop captions", "Altyazıları durdur", "Detener subtítulos", "Parar legendas", "停止字幕"],
    Welcome => [
        "Hammurabi runs from the tray icon. Closing this window keeps it there. Pick what to listen to, then press Start captions.",
        "Hammurabi, sistem tepsisindeki simgesinden çalışır. Bu pencereyi kapatsanız da orada çalışmaya devam eder. Neyin dinleneceğini seçin, ardından Altyazıları başlat'a basın.",
        "Hammurabi funciona desde el icono de la bandeja del sistema. Si cierras esta ventana, sigue ahí. Elige qué escuchar y pulsa Iniciar subtítulos.",
        "O Hammurabi fica no ícone da bandeja do sistema. Fechar esta janela não o encerra. Escolha o que ouvir e clique em Iniciar legendas.",
        "Hammurabi 在系统托盘图标中运行，关闭此窗口后它仍会留在那里。选择要收听的内容，然后点击“开始字幕”。",
    ],
    GotIt => ["Got it", "Anladım", "Entendido", "Entendi", "知道了"],

    // Shortcut
    PressKeys => ["Press the keys\u{2026}", "Tuşlara basın\u{2026}", "Pulsa las teclas\u{2026}", "Pressione as teclas\u{2026}", "请按下按键\u{2026}"],
    HotkeyRules => [
        "Ctrl or Alt with an F key, or two of Ctrl, Alt and Shift with a letter, digit or Space. Esc cancels.",
        "Bir F tuşuyla Ctrl ya da Alt, veya bir harf, rakam ya da Boşluk ile Ctrl, Alt ve Shift'ten ikisi. Esc iptal eder.",
        "Ctrl o Alt con una tecla F, o dos de Ctrl, Alt y Mayús con una letra, un número o Espacio. Esc cancela.",
        "Ctrl ou Alt com uma tecla F, ou duas entre Ctrl, Alt e Shift com uma letra, um número ou Espaço. Esc cancela.",
        "Ctrl 或 Alt 加一个 F 键，或 Ctrl、Alt、Shift 中任意两个加字母、数字或空格键。按 Esc 取消。",
    ],
    HotkeyClick => ["Click, then press the new shortcut", "Tıklayın, ardından yeni kısayola basın", "Haz clic y pulsa el nuevo atajo", "Clique e pressione o novo atalho", "点击后按下新的快捷键"],

    // General page
    SectionApp => ["App", "Uygulama", "Aplicación", "Aplicativo", "应用"],
    Language => ["Language", "Dil", "Idioma", "Idioma", "语言"],
    SystemLanguage => ["System language ({0})", "Sistem dili ({0})", "Idioma del sistema ({0})", "Idioma do sistema ({0})", "跟随系统（{0}）"],
    Theme => ["Theme", "Tema", "Tema", "Tema", "主题"],
    ThemeSystem => ["System", "Sistem", "Sistema", "Sistema", "跟随系统"],
    ThemeDark => ["Dark", "Koyu", "Oscuro", "Escuro", "深色"],
    ThemeLight => ["Light", "Açık", "Claro", "Claro", "浅色"],
    StartAtLoginLong => ["Start Hammurabi at login", "Oturum açılınca Hammurabi'yi başlat", "Abrir Hammurabi al iniciar sesión", "Abrir o Hammurabi ao entrar no sistema", "登录时启动 Hammurabi"],
    UpdateAutomatically => ["Update automatically", "Otomatik güncelle", "Actualizar automáticamente", "Atualizar automaticamente", "自动更新"],
    UpdateAutomaticallyHint => [
        "Checks GitHub for a new version every few hours and installs it while captions are off. Nothing else is sent.",
        "Birkaç saatte bir GitHub'da yeni sürüm olup olmadığına bakar ve altyazılar kapalıyken kurar. Başka hiçbir şey gönderilmez.",
        "Busca una versión nueva en GitHub cada pocas horas y la instala mientras los subtítulos están desactivados. No se envía nada más.",
        "Procura uma versão nova no GitHub a cada poucas horas e a instala enquanto as legendas estão desligadas. Nada mais é enviado.",
        "每隔几小时在 GitHub 上检查一次新版本，并在字幕关闭时安装。不会发送任何其他信息。",
    ],
    CheckUpdates => ["Check for updates", "Güncellemeleri denetle", "Buscar actualizaciones", "Verificar atualizações", "检查更新"],
    CheckingUpdates => ["Checking for updates...", "Güncellemeler denetleniyor\u{2026}", "Buscando actualizaciones\u{2026}", "Verificando atualizações\u{2026}", "正在检查更新\u{2026}"],
    UpToDate => ["Hammurabi is up to date.", "Hammurabi güncel.", "Hammurabi está actualizado.", "O Hammurabi está atualizado.", "Hammurabi 已是最新版本。"],
    DownloadingUpdate => ["Downloading v{0}: {1}%", "v{0} indiriliyor: %{1}", "Descargando la v{0}: {1}%", "Baixando a v{0}: {1}%", "正在下载 v{0}：{1}%"],
    UpdateReady => [
        "v{0} is ready. It installs when captions are off.",
        "v{0} hazır. Altyazılar kapalıyken kurulacak.",
        "La v{0} está lista. Se instalará cuando los subtítulos estén desactivados.",
        "A v{0} está pronta. Ela será instalada quando as legendas estiverem desligadas.",
        "v{0} 已就绪，将在字幕关闭时安装。",
    ],
    UpdateOut => ["v{0} is out. Get it", "v{0} çıktı. İndirin", "Ya salió la v{0}. Descárgala", "A v{0} saiu. Baixe agora", "v{0} 已发布，点此获取"],
    UpdateCheckFailed => [
        "Could not check for updates. Try again later.",
        "Güncellemeler denetlenemedi. Daha sonra yeniden deneyin.",
        "No se pudieron buscar actualizaciones. Inténtalo más tarde.",
        "Não foi possível verificar atualizações. Tente de novo mais tarde.",
        "无法检查更新，请稍后再试。",
    ],
    UpdatedTo => ["Hammurabi is updated to v{0}.", "Hammurabi, v{0} sürümüne güncellendi.", "Hammurabi se actualizó a la v{0}.", "O Hammurabi foi atualizado para a v{0}.", "Hammurabi 已更新到 v{0}。"],
    UpdatingTo => [
        "Updating Hammurabi to v{0}. It restarts in a moment.",
        "Hammurabi v{0} sürümüne güncelleniyor. Birazdan yeniden başlayacak.",
        "Actualizando Hammurabi a la v{0}. Se reiniciará en un momento.",
        "Atualizando o Hammurabi para a v{0}. Ele reinicia em instantes.",
        "正在将 Hammurabi 更新到 v{0}，稍后会重新启动。",
    ],
    SwitchingToGpu => [
        "Switching to the RTX 50 version. Hammurabi restarts in a moment.",
        "RTX 50 sürümüne geçiliyor. Hammurabi birazdan yeniden başlayacak.",
        "Cambiando a la versión RTX 50. Hammurabi se reiniciará en un momento.",
        "Mudando para a versão RTX 50. O Hammurabi reinicia em instantes.",
        "正在切换到 RTX 50 版本，Hammurabi 稍后会重新启动。",
    ],
    NowOnGpu => ["Hammurabi now listens on your graphics card.", "Hammurabi artık ekran kartınızla dinliyor.", "Hammurabi ahora escucha con tu tarjeta gráfica.", "O Hammurabi agora escuta com a sua placa de vídeo.", "Hammurabi 现在用你的显卡收听。"],
    // {0} is the graphics card's name.
    GpuBuildHelps => [
        "Your {0} hears much faster with the RTX 50 version of Hammurabi. Your settings and models stay.",
        "{0} kartınız, Hammurabi'nin RTX 50 sürümüyle konuşmayı çok daha hızlı duyar. Ayarlarınız ve modelleriniz kalır.",
        "Tu {0} reconoce la voz mucho más rápido con la versión RTX 50 de Hammurabi. Se conservan tus ajustes y modelos.",
        "Sua {0} reconhece a fala muito mais rápido com a versão RTX 50 do Hammurabi. Suas configurações e modelos continuam.",
        "使用 Hammurabi 的 RTX 50 版本，你的 {0} 识别语音会快得多。设置和模型都会保留。",
    ],
    SwitchToGpu => ["Switch to the RTX 50 version", "RTX 50 sürümüne geç", "Cambiar a la versión RTX 50", "Mudar para a versão RTX 50", "切换到 RTX 50 版本"],
    GetGpuBuild => ["Download the RTX 50 version", "RTX 50 sürümünü indir", "Descargar la versión RTX 50", "Baixar a versão RTX 50", "下载 RTX 50 版本"],
    Shortcuts => ["Shortcuts", "Kısayollar", "Atajos", "Atalhos", "快捷键"],
    StartStopCaptions => ["Start / stop captions", "Altyazıları başlat / durdur", "Iniciar / detener subtítulos", "Iniciar / parar legendas", "开始 / 停止字幕"],
    Models => ["Models", "Modeller", "Modelos", "Modelos", "模型"],
    GroupSpeech => ["Speech", "Konuşma", "Voz", "Fala", "语音识别"],
    GroupTranslation => ["Translation", "Çeviri", "Traducción", "Tradução", "翻译"],
    GroupVoices => ["Voices", "Sesler", "Voces", "Vozes", "朗读声音"],
    AgreeLicenceFirst => ["Agree to its licence in Read aloud first", "Önce Sesli okuma sayfasında lisansını kabul edin", "Acepta primero su licencia en Leer en voz alta", "Aceite primeiro a licença em Ler em voz alta", "请先在“朗读”页面同意其许可"],
    Files => ["Files", "Dosyalar", "Archivos", "Arquivos", "文件"],
    OpenModelsFolder => ["Open models folder", "Modeller klasörünü aç", "Abrir carpeta de modelos", "Abrir pasta de modelos", "打开模型文件夹"],
    OpenLogs => ["Open logs", "Günlükleri aç", "Abrir registros", "Abrir registros", "打开日志"],
    RemoveWord => ["Remove {0}", "{0} kelimesini kaldır", "Quitar {0}", "Remover {0}", "移除 {0}"],

    // Downloads
    DownloadNoteSpeech => [
        "Starting downloads the speech model first, {0}, once.",
        "Başlatınca önce konuşma modeli indirilir ({0}, yalnızca bir kez).",
        "Al iniciar se descarga primero el modelo de voz ({0}, una sola vez).",
        "Ao iniciar, o modelo de fala é baixado primeiro ({0}, só uma vez).",
        "首次开始时会先下载语音识别模型（{0}，仅需一次）。",
    ],
    DownloadNoteTranslation => [
        "Starting downloads the translation model first, {0}, once.",
        "Başlatınca önce çeviri modeli indirilir ({0}, yalnızca bir kez).",
        "Al iniciar se descarga primero el modelo de traducción ({0}, una sola vez).",
        "Ao iniciar, o modelo de tradução é baixado primeiro ({0}, só uma vez).",
        "首次开始时会先下载翻译模型（{0}，仅需一次）。",
    ],
    DownloadNoteBoth => [
        "Starting downloads the speech and translation models first, {0}, once.",
        "Başlatınca önce konuşma ve çeviri modelleri indirilir ({0}, yalnızca bir kez).",
        "Al iniciar se descargan primero los modelos de voz y de traducción ({0}, una sola vez).",
        "Ao iniciar, os modelos de fala e de tradução são baixados primeiro ({0}, só uma vez).",
        "首次开始时会先下载语音识别和翻译模型（{0}，仅需一次）。",
    ],
    MbOf => ["{0} of {1} MB", "{0} / {1} MB", "{0} de {1} MB", "{0} de {1} MB", "{0} / {1} MB"],
    Downloaded => ["Downloaded", "İndirildi", "Descargado", "Baixado", "已下载"],
    InUse => ["in use", "kullanımda", "en uso", "em uso", "使用中"],
    InUseHint => [
        "Captions are using it. Stop them to delete it.",
        "Altyazılar bunu kullanıyor. Silmek için altyazıları durdurun.",
        "Los subtítulos lo están usando. Detenlos para eliminarlo.",
        "As legendas estão usando este modelo. Pare-as para excluí-lo.",
        "字幕正在使用它。请先停止字幕再删除。",
    ],
    DeleteModelQuestion => ["Delete {0}?", "{0} silinsin mi?", "¿Eliminar {0}?", "Excluir {0}?", "删除{0}？"],
    DeleteFrees => [
        "This frees {0}. You can download it again later.",
        "Bu, diskte {0} yer açar. Daha sonra yeniden indirebilirsiniz.",
        "Esto libera {0}. Puedes volver a descargarlo más tarde.",
        "Isso libera {0}. Você pode baixá-lo de novo depois.",
        "这将释放 {0}。之后可以重新下载。",
    ],
    Delete => ["Delete", "Sil", "Eliminar", "Excluir", "删除"],
    Cancel => ["Cancel", "İptal", "Cancelar", "Cancelar", "取消"],
    StartingDownload => ["Starting download", "İndirme başlıyor", "Iniciando la descarga", "Iniciando o download", "正在开始下载"],
    Recommended => ["Recommended for your PC", "Bilgisayarınız için önerilen", "Recomendado para tu equipo", "Recomendado para o seu PC", "推荐用于你的电脑"],
    Download => ["Download", "İndir", "Descargar", "Baixar", "下载"],
    DownloadFirst => ["Download first", "Önce indirin", "Descarga primero", "Baixe primeiro", "请先下载"],

    // Model names
    FastTranslator => ["Fast translator", "Hızlı çevirici", "Traductor rápido", "Tradutor rápido", "快速翻译器"],
    NaturalTranslator => ["Natural translator", "Doğal çevirici", "Traductor natural", "Tradutor natural", "自然翻译器"],
    NaturalTranslatorSmaller => ["Natural translator, smaller", "Doğal çevirici, küçük", "Traductor natural, más pequeño", "Tradutor natural, menor", "自然翻译器（小）"],
    NaturalVoices => ["Natural voices", "Doğal sesler", "Voces naturales", "Vozes naturais", "自然语音"],
    SpeakersVoice => ["Speaker's voice", "Konuşanın sesi", "Voz del hablante", "Voz de quem fala", "说话人的声音"],

    // Listen page
    CaptionsFrom => ["Captions from", "Altyazı kaynağı", "Subtítulos desde", "Legendas a partir de", "字幕来源"],
    Listening => ["Listening", "Dinleme", "Escuchando", "Ouvindo", "收听"],
    WindowsLiveCaptions => ["Windows Live Captions", "Windows canlı açıklamalı alt yazıları", "Subtítulos en directo de Windows", "Legendas ao Vivo do Windows", "Windows 实时字幕"],
    SubtitlesOnScreen => ["Subtitles on screen", "Ekrandaki altyazılar", "Subtítulos en pantalla", "Legendas na tela", "屏幕上的字幕"],
    ScreenHint => [
        "Does not listen: reads the subtitles a game or video shows and translates those. For a game with subtitles in another language.",
        "Dinlemez: bir oyunun ya da videonun gösterdiği altyazıları okuyup çevirir. Altyazısı başka dilde olan oyunlar için.",
        "No escucha: lee los subtítulos que muestra un juego o vídeo y los traduce. Para juegos con subtítulos en otro idioma.",
        "Não escuta: lê as legendas que um jogo ou vídeo mostra e as traduz. Para jogos com legendas em outro idioma.",
        "不收听声音：读取游戏或视频显示的字幕并翻译。适合字幕是其他语言的游戏。",
    ],
    ReadsSubtitlesNote => ["Reads the subtitles a game shows.", "Bir oyunun gösterdiği altyazıları okur.", "Lee los subtítulos que muestra un juego.", "Lê as legendas que um jogo mostra.", "读取游戏显示的字幕。"],
    LiveCaptionsNote => [
        "Language and microphone come from Windows Live Captions settings.",
        "Dil ve mikrofon, Windows canlı açıklamalı alt yazı ayarlarından alınır.",
        "El idioma y el micrófono se toman de la configuración de Subtítulos en directo de Windows.",
        "O idioma e o microfone vêm das configurações de Legendas ao Vivo do Windows.",
        "语言和麦克风取自 Windows 实时字幕的设置。",
    ],
    ListensNote => ["Listens to the computer's sound or a microphone.", "Bilgisayarın sesini ya da bir mikrofonu dinler.", "Escucha el sonido del equipo o un micrófono.", "Ouve o som do computador ou um microfone.", "收听电脑的声音或麦克风。"],
    WhatToCaption => ["What to caption", "Dinlenecek ses", "Qué subtitular", "O que legendar", "收听什么"],
    SystemAudio => ["System audio", "Sistem sesi", "Audio del sistema", "Áudio do sistema", "系统声音"],
    Microphone => ["Microphone", "Mikrofon", "Micrófono", "Microfone", "麦克风"],
    Both => ["Both", "İkisi de", "Ambos", "Ambos", "两者"],
    SystemAudioUnavailable => ["System audio unavailable: {0}", "Sistem sesi kullanılamıyor: {0}", "Audio del sistema no disponible: {0}", "Áudio do sistema indisponível: {0}", "系统声音不可用：{0}"],
    SystemAudioFrom => ["System audio from", "Sistem sesinin kaynağı", "Audio del sistema desde", "Áudio do sistema de", "系统声音来源"],
    Refresh => ["Refresh", "Yenile", "Actualizar", "Atualizar", "刷新"],
    MicOrCaptureCard => ["Microphone or capture card", "Mikrofon veya yakalama kartı", "Micrófono o capturadora", "Microfone ou placa de captura", "麦克风或采集卡"],
    Default => ["Default", "Varsayılan", "Predeterminado", "Padrão", "默认"],
    Missing => ["{0} (missing)", "{0} (bulunamadı)", "{0} (no encontrado)", "{0} (não encontrado)", "{0}（未找到）"],

    // Subtitles on screen
    SubtitleLanguage => ["Subtitle language", "Altyazı dili", "Idioma de los subtítulos", "Idioma das legendas", "字幕语言"],
    WindowsCannotRead => [
        "Windows cannot read {0} yet. Add it in Windows Settings > Time & language > Language & region.",
        "Windows bu dili ({0}) henüz okuyamıyor. Windows Ayarlar > Saat ve dil > Dil ve bölge bölümünden ekleyin.",
        "Windows aún no puede leer este idioma ({0}). Añádelo en Configuración de Windows > Hora e idioma > Idioma y región.",
        "O Windows ainda não consegue ler este idioma ({0}). Adicione-o em Configurações do Windows > Hora e idioma > Idioma e região.",
        "Windows 还无法读取该语言（{0}）。请在 Windows 设置 > 时间和语言 > 语言和区域 中添加。",
    ],
    ReadFrom => ["Read from", "Okunacak yer", "Leer de", "Ler de", "读取来源"],
    MainDisplay => ["Main display", "Ana ekran", "Pantalla principal", "Tela principal", "主显示器"],
    ReadFromHint => [
        "A window is read even when others cover it, but not while it is minimized. For a console on a capture card, pick OBS's projector window of the game.",
        "Bir pencere, üstünü başka pencereler kapatsa da okunur, ama simge durumuna küçültülmüşken okunmaz. Yakalama kartına bağlı bir konsol için OBS'teki oyunun projektör penceresini seçin.",
        "Una ventana se lee aunque otras la tapen, pero no mientras está minimizada. Para una consola en una capturadora, elige la ventana de proyector del juego en OBS.",
        "Uma janela é lida mesmo quando outras a cobrem, mas não enquanto está minimizada. Para um console em uma placa de captura, escolha a janela de projetor do jogo no OBS.",
        "即使窗口被其他窗口遮挡也能读取，但最小化时不能。若游戏机通过采集卡连接，请选择 OBS 中该游戏的投影窗口。",
    ],
    SubtitleArea => ["Subtitle area", "Altyazı alanı", "Zona de subtítulos", "Área das legendas", "字幕区域"],
    AreaFrom => ["from", "üst", "de", "de", "从"],
    AreaTo => ["to", "alt", "a", "até", "到"],
    SubtitleAreaHint => [
        "How far down the picture the subtitles are. The bottom quarter suits most games.",
        "Altyazıların görüntünün ne kadar aşağısında olduğu. Çoğu oyun için alttaki dörtte bir uygundur.",
        "A qué altura de la imagen están los subtítulos. El cuarto inferior sirve para la mayoría de los juegos.",
        "Em que altura da imagem ficam as legendas. O quarto inferior serve para a maioria dos jogos.",
        "字幕在画面中的上下位置。大多数游戏选底部四分之一即可。",
    ],
    WaitingForPicture => ["Waiting for a picture of it.", "Görüntüsü bekleniyor.", "Esperando una imagen.", "Aguardando uma imagem.", "正在等待画面。"],
    LitPartNote => [
        "The lit part is read. Drag its top and bottom around the subtitles, and its sides to the video's when that does not fill the window.",
        "Aydınlık kısım okunur. Üst ve alt kenarlarını altyazıları içine alacak şekilde sürükleyin; video pencereyi doldurmuyorsa yan kenarlarını da videonun kenarlarına çekin.",
        "Se lee la parte iluminada. Arrastra sus bordes superior e inferior alrededor de los subtítulos, y los laterales a los del vídeo si este no llena la ventana.",
        "A parte iluminada é lida. Arraste as bordas de cima e de baixo em volta das legendas, e as laterais até as do vídeo quando ele não preencher a janela.",
        "亮起的部分会被读取。拖动其上下边缘框住字幕；视频未填满窗口时，再把左右边缘拖到视频边缘。",
    ],
    CoverSubtitles => ["Cover the game's subtitles", "Oyunun altyazısını kapat", "Tapar los subtítulos del juego", "Cobrir as legendas do jogo", "遮住游戏的字幕"],
    CoverSubtitlesHint => [
        "The translation stands right over the game's own subtitle and hides it, so only the translation shows: on the screen and on the captions web page.",
        "Çeviri, oyunun kendi altyazısının tam üstünde durur ve onu gizler; böylece hem ekranda hem altyazı web sayfasında yalnızca çeviri görünür.",
        "La traducción se pone justo encima del subtítulo del juego y lo oculta, así solo se ve la traducción: en pantalla y en la página web de subtítulos.",
        "A tradução fica bem em cima da legenda do próprio jogo e a esconde, então só a tradução aparece: na tela e na página web de legendas.",
        "译文正好盖在游戏自带的字幕上并将其遮住，屏幕上和字幕网页上都只显示译文。",
    ],
    WindowNotOpen => ["That window is not open, or is minimized.", "Bu pencere açık değil ya da simge durumuna küçültülmüş.", "Esa ventana no está abierta o está minimizada.", "Essa janela não está aberta ou está minimizada.", "该窗口未打开或已最小化。"],

    // Speech recognition
    SpeechRecognition => ["Speech recognition", "Konuşma tanıma", "Reconocimiento de voz", "Reconhecimento de fala", "语音识别"],
    SpokenLanguage => ["Spoken language", "Konuşulan dil", "Idioma hablado", "Idioma falado", "说话语言"],
    AutoDetect => ["Auto-detect", "Otomatik algıla", "Detectar automáticamente", "Detectar automaticamente", "自动检测"],
    WordsToExpect => ["Words to expect", "Geçecek kelimeler", "Palabras esperadas", "Palavras esperadas", "预期词语"],
    HintPlaceholder => ["Add a word", "Kelime ekle", "Añadir una palabra", "Adicionar uma palavra", "添加词语"],
    HintHover => [
        "Helps the speech model hear and spell names, places and slang. Several at once go in with commas between them. Not used while subtitles are read off the screen.",
        "Konuşma modelinin adları, yerleri ve argoyu doğru duyup doğru yazmasına yardım eder. Birkaçını aralarına virgül koyarak bir kerede ekleyebilirsiniz. Altyazılar ekrandan okunurken kullanılmaz.",
        "Ayuda al modelo de voz a oír y escribir bien nombres, lugares y jerga. Puedes añadir varios a la vez separados por comas. No se usa mientras se leen subtítulos de la pantalla.",
        "Ajuda o modelo de fala a ouvir e escrever nomes, lugares e gírias. Vários de uma vez entram separados por vírgulas. Não é usado enquanto as legendas são lidas da tela.",
        "帮助语音识别模型听清并正确写出人名、地名和俚语。可一次输入多个，用逗号隔开。从屏幕读取字幕时不使用。",
    ],
    OneWord => ["1 word", "1 kelime", "1 palabra", "1 palavra", "1 个词"],
    NWords => ["{0} words", "{0} kelime", "{0} palabras", "{0} palavras", "{0} 个词"],
    Model => ["Model", "Model", "Modelo", "Modelo", "模型"],
    AutoModel => ["Auto ({0})", "Otomatik ({0})", "Automático ({0})", "Automático ({0})", "自动（{0}）"],
    KeepingUp => ["Keeping up", "Hıza yetişiyor mu", "¿Sigue el ritmo?", "Acompanha?", "能否跟上"],
    Speed => ["Speed", "Hız", "Velocidad", "Velocidade", "速度"],
    SkipSilence => ["Skip the silence around speech", "Konuşmanın öncesindeki ve sonrasındaki sessizliği atla", "Omitir el silencio alrededor de la voz", "Pular o silêncio em volta da fala", "跳过语音前后的静音"],
    SkipSilenceHint => [
        "Much faster on a computer without a graphics card. Turn it off if captions get worse.",
        "Ekran kartı olmayan bilgisayarlarda çok daha hızlıdır. Altyazılar kötüleşirse kapatın.",
        "Mucho más rápido en un equipo sin tarjeta gráfica. Desactívalo si los subtítulos empeoran.",
        "Muito mais rápido em um computador sem placa de vídeo. Desligue se as legendas piorarem.",
        "在没有显卡的电脑上快得多。如果字幕变差，请关闭。",
    ],
    RunsOn => ["Runs on", "Çalıştığı donanım","Se ejecuta en", "Roda em", "运行于"],
    RunsOnCpu => [
        "The processor. Listening uses a graphics card only in the RTX 50 download. Tiny and Base keep up best.",
        "İşlemci. Dinleme, ekran kartını yalnızca RTX 50 indirmesinde kullanır. Hıza en iyi Tiny ve Base yetişir.",
        "El procesador. Escuchar solo usa la tarjeta gráfica en la descarga para RTX 50. Tiny y Base son los que mejor siguen el ritmo.",
        "O processador. A escuta só usa a placa de vídeo no download para RTX 50. Tiny e Base acompanham melhor.",
        "处理器。只有 RTX 50 版下载包才用显卡收听。Tiny 和 Base 最能跟上。",
    ],
    RunsOnMetal => ["This Mac's graphics chip", "Bu Mac'in grafik işlemcisi", "El chip gráfico de este Mac", "O chip gráfico deste Mac", "这台 Mac 的图形芯片"],
    RunsOnNvidia => ["Your NVIDIA graphics card", "NVIDIA ekran kartınız", "Tu tarjeta gráfica NVIDIA", "Sua placa de vídeo NVIDIA", "你的 NVIDIA 显卡"],

    // Keeping up: {0} is the time, {1} VerbHear or VerbTranslate, {2} what helps.
    ShownWhileRunning => ["Shown while captions run.", "Altyazılar çalışırken gösterilir.", "Se muestra mientras los subtítulos están activos.", "Aparece enquanto as legendas estão ligadas.", "字幕运行时显示。"],
    KeepUpYes => ["Yes: a line takes {0} to {1}", "Evet: bir satırı {1} {0} sürüyor", "Sí: una línea tarda {0} en {1}", "Sim: uma linha leva {0} para ser {1}", "能：每行{1}需要 {0}"],
    KeepUpNearly => ["Nearly: a line takes {0} to {1}", "Neredeyse: bir satırı {1} {0} sürüyor", "Casi: una línea tarda {0} en {1}", "Quase: uma linha leva {0} para ser {1}", "勉强：每行{1}需要 {0}"],
    KeepUpNo => ["No: a line takes {0} to {1}. {2}", "Hayır: bir satırı {1} {0} sürüyor. {2}", "No: una línea tarda {0} en {1}. {2}", "Não: uma linha leva {0} para ser {1}. {2}", "不能：每行{1}需要 {0}。{2}"],
    VerbHear => ["hear", "duymak", "reconocerse", "reconhecida", "识别"],
    VerbTranslate => ["translate", "çevirmek", "traducirse", "traduzida", "翻译"],
    Seconds => ["{0} s", "{0} sn", "{0} s", "{0} s", "{0} 秒"],
    Percent => ["{0}%", "%{0}", "{0}%", "{0}%", "{0}%"],
    AdviceSmallerModel => ["A smaller model keeps up better.", "Daha küçük bir model hıza daha iyi yetişir.", "Un modelo más pequeño sigue mejor el ritmo.", "Um modelo menor acompanha melhor.", "较小的模型更能跟上。"],
    AdviceTooSlowScreen => [
        "This computer is too slow to listen. Try Subtitles on screen.",
        "Bu bilgisayar dinlemek için fazla yavaş. Ekrandaki altyazıları deneyin.",
        "Este equipo es demasiado lento para escuchar. Prueba Subtítulos en pantalla.",
        "Este computador é lento demais para ouvir. Experimente Legendas na tela.",
        "这台电脑太慢，无法收听。试试“屏幕上的字幕”。",
    ],
    AdviceTooSlow => ["This computer is too slow to listen.", "Bu bilgisayar dinlemek için fazla yavaş.", "Este equipo es demasiado lento para escuchar.", "Este computador é lento demais para ouvir.", "这台电脑太慢，无法收听。"],
    AdviceGpuBusySmaller => ["The graphics card is busy. The smaller model needs less.", "Ekran kartı meşgul. Küçük model daha azına ihtiyaç duyar.", "La tarjeta gráfica está ocupada. El modelo más pequeño necesita menos.", "A placa de vídeo está ocupada. O modelo menor precisa de menos.", "显卡正忙。较小的模型占用更少。"],
    AdviceGpuBusy => ["The graphics card is busy.", "Ekran kartı meşgul.", "La tarjeta gráfica está ocupada.", "A placa de vídeo está ocupada.", "显卡正忙。"],
    AdviceCpuSlow => ["This computer is slow for it.", "Bu bilgisayar bunun için yavaş.", "Este equipo es lento para esto.", "Este computador é lento para isso.", "这台电脑运行它太慢。"],
    AdviceServerSlow => ["Your server is slow.", "Sunucunuz yavaş.", "Tu servidor es lento.", "Seu servidor está lento.", "你的服务器太慢。"],

    // Translate page
    TranslateCaptions => ["Translate captions", "Altyazıları çevir", "Traducir subtítulos", "Traduzir legendas", "翻译字幕"],
    TranslateTo => ["Translate to", "Hedef dil", "Traducir a", "Traduzir para", "翻译为"],
    Translator => ["Translator", "Çevirici", "Traductor", "Tradutor", "翻译器"],
    Fast => ["Fast", "Hızlı", "Rápido", "Rápido", "快速"],
    FastNote => ["Word for word. Runs on any computer.", "Kelimesi kelimesine çevirir. Her bilgisayarda çalışır.", "Palabra por palabra. Funciona en cualquier equipo.", "Palavra por palavra. Roda em qualquer computador.", "逐字翻译，任何电脑都能运行。"],
    Natural => ["Natural", "Doğal", "Natural", "Natural", "自然"],
    NaturalNote => [
        "Understands slang, idiom and tone. Needs a graphics card with 8 GB of memory free, or 3.5 GB with the smaller model.",
        "Argoyu, deyimleri ve tonu anlar. 8 GB boş belleği olan bir ekran kartı ister; küçük modelle 3,5 GB yeter.",
        "Entiende jerga, modismos y tono. Necesita una tarjeta gráfica con 8 GB de memoria libre, o 3,5 GB con el modelo más pequeño.",
        "Entende gírias, expressões e tom. Precisa de uma placa de vídeo com 8 GB de memória livre, ou 3,5 GB com o modelo menor.",
        "能理解俚语、习语和语气。需要显卡有 8 GB 可用显存，较小的模型需要 3.5 GB。",
    ],
    OwnServer => ["Your own AI server (advanced)", "Kendi yapay zekâ sunucunuz (ileri düzey)", "Tu propio servidor de IA (avanzado)", "Seu próprio servidor de IA (avançado)", "你自己的 AI 服务器（高级）"],
    OwnServerNote => ["Ollama, LM Studio or any OpenAI-compatible server.", "Ollama, LM Studio ya da OpenAI uyumlu herhangi bir sunucu.", "Ollama, LM Studio o cualquier servidor compatible con OpenAI.", "Ollama, LM Studio ou qualquer servidor compatível com OpenAI.", "Ollama、LM Studio 或任何兼容 OpenAI 的服务器。"],
    SmallerModel => ["Smaller model", "Küçük model", "Modelo más pequeño", "Modelo menor", "较小的模型"],
    SmallerModelHint => [
        "Takes 3.5 GB of graphics memory instead of 8, and translates a little less naturally. For when a game runs on the same card.",
        "8 yerine 3,5 GB ekran kartı belleği kullanır, çevirisi biraz daha az doğal olur. Aynı kartta oyun da çalışıyorsa bunu seçin.",
        "Usa 3,5 GB de memoria gráfica en vez de 8 y traduce con algo menos de naturalidad. Para cuando un juego usa la misma tarjeta.",
        "Usa 3,5 GB de memória de vídeo em vez de 8 e traduz de forma um pouco menos natural. Para quando um jogo roda na mesma placa.",
        "占用 3.5 GB 显存而不是 8 GB，翻译稍欠自然。适合游戏和它共用同一张显卡时。",
    ],
    Address => ["Address", "Adres", "Dirección", "Endereço", "地址"],
    ModelName => ["Model name", "Model adı", "Nombre del modelo", "Nome do modelo", "模型名称"],
    TranslateWhileSaid => ["Translate while a sentence is still being said", "Cümle bitmeden çevir", "Traducir mientras aún se dice la frase", "Traduzir enquanto a frase ainda está sendo dita", "句子没说完就开始翻译"],
    SoonerNote => ["Sooner, but the words change as it grows.", "Çeviri daha erken gelir, ama cümle uzadıkça kelimeler değişir.", "Más pronto, pero las palabras cambian a medida que la frase crece.", "Sai antes, mas as palavras mudam conforme a frase cresce.", "译文出现得更快，但词语会随句子变长而改变。"],

    // Read aloud page
    ReadCaptionsAloud => ["Read captions aloud", "Altyazıları sesli oku", "Leer los subtítulos en voz alta", "Ler as legendas em voz alta", "朗读字幕"],
    ReadCaptionsAloudHint => [
        "Speaks each finished caption, the translation when there is one.",
        "Tamamlanan her altyazıyı, varsa çevirisini sesli okur.",
        "Lee cada subtítulo terminado, la traducción si la hay.",
        "Lê cada legenda concluída, a tradução quando houver.",
        "朗读每条完成的字幕，有译文时朗读译文。",
    ],
    ReadingWith => ["Reading with {0}", "{0} ile okunuyor", "Leyendo con {0}", "Lendo com {0}", "正在用{0}朗读"],
    VoiceEngine => ["Voice engine", "Ses motoru", "Motor de voz", "Mecanismo de voz", "语音引擎"],
    SystemVoices => ["System voices", "Sistem sesleri", "Voces del sistema", "Vozes do sistema", "系统语音"],
    SystemVoicesNote => ["The voices installed on this computer.", "Bu bilgisayarda yüklü sesler.", "Las voces instaladas en este equipo.", "As vozes instaladas neste computador.", "这台电脑上安装的语音。"],
    NaturalVoicesNote => [
        "Voices that run on this computer, for 22 languages. Others use system voices.",
        "Bu bilgisayarda çalışan, 22 dili konuşan sesler. Diğer diller sistem sesleriyle okunur.",
        "Voces que se ejecutan en este equipo, para 22 idiomas. Los demás usan voces del sistema.",
        "Vozes que rodam neste computador, para 22 idiomas. Os outros usam vozes do sistema.",
        "在本机运行的语音，支持 22 种语言。其他语言使用系统语音。",
    ],
    SystemVoicesUntilDownloaded => ["System voices read until it is downloaded.", "İndirilene kadar sistem sesleri okur.", "Las voces del sistema leen hasta que se descargue.", "As vozes do sistema leem até o download terminar.", "下载完成前由系统语音朗读。"],
    GpuDownloadOnly => ["Comes with the hammurabi-windows-gpu download", "hammurabi-windows-gpu indirmesiyle gelir", "Viene con la descarga hammurabi-windows-gpu", "Vem com o download hammurabi-windows-gpu", "随 hammurabi-windows-gpu 下载包提供"],
    SpeakerVoiceNoteGpu => [
        "Reads in the voice of whoever is speaking. Comes with the hammurabi-windows-gpu download.",
        "Konuşan kişinin sesiyle okur. hammurabi-windows-gpu indirmesiyle gelir.",
        "Lee con la voz de quien está hablando. Viene con la descarga hammurabi-windows-gpu.",
        "Lê com a voz de quem está falando. Vem com o download hammurabi-windows-gpu.",
        "用说话人的声音朗读。随 hammurabi-windows-gpu 下载包提供。",
    ],
    SpeakerVoiceNote => [
        "Reads in the voice of whoever is speaking, learned on this computer from a few seconds of their speech. Needs a graphics card.",
        "Konuşan kişinin sesiyle okur; ses, birkaç saniyelik konuşmasından bu bilgisayarda öğrenilir. Ekran kartı gerekir.",
        "Lee con la voz de quien está hablando, aprendida en este equipo a partir de unos segundos de su voz. Necesita una tarjeta gráfica.",
        "Lê com a voz de quem está falando, aprendida neste computador a partir de alguns segundos da fala. Precisa de uma placa de vídeo.",
        "用说话人的声音朗读，声音由本机从几秒钟的讲话中学会。需要显卡。",
    ],
    BackupVoice => ["Backup voice", "Yedek ses", "Voz de respaldo", "Voz reserva", "备用声音"],
    Voice => ["Voice", "Ses", "Voz", "Voz", "声音"],
    BackupVoiceNote => [
        "Reads until the speaker's voice is learned, and if it cannot run.",
        "Konuşanın sesi öğrenilene kadar ve o çalışamadığında okur.",
        "Lee hasta que se aprende la voz del hablante, y cuando esta no puede ejecutarse.",
        "Lê até a voz de quem fala ser aprendida, e quando ela não puder rodar.",
        "在学会说话人的声音之前，或其无法运行时朗读。",
    ],
    NaturalVoiceN => ["Natural voice {0}", "Doğal ses {0}", "Voz natural {0}", "Voz natural {0}", "自然语音 {0}"],
    SystemVoice => ["System voice", "Sistem sesi", "Voz del sistema", "Voz do sistema", "系统语音"],
    NoVoiceFor => ["No {0} voice", "{0} için ses yok", "No hay voz para {0}", "Sem voz para {0}", "没有 {0} 语音"],
    Playback => ["Playback", "Oynatma", "Reproducción", "Reprodução", "播放"],
    Volume => ["Volume", "Ses düzeyi", "Volumen", "Volume", "音量"],
    PlayOn => ["Play on", "Çalınacak cihaz", "Reproducir en", "Tocar em", "播放设备"],
    OtherAppsWhileReading => ["Other apps while reading", "Okurken diğer uygulamalar", "Otras apps durante la lectura", "Outros apps durante a leitura", "朗读时其他应用"],
    NotTurnedDown => ["Not turned down", "Kısılmaz", "Sin bajar", "Sem abaixar", "不降低"],
    AlsoReadSame => [
        "Also read speech already in {0}",
        "Zaten hedef dilde ({0}) olan konuşmayı da oku",
        "Leer también lo que ya está en el idioma de destino ({0})",
        "Ler também o que já está no idioma de destino ({0})",
        "也朗读本来就是目标语言（{0}）的语音",
    ],
    AlsoReadSameHint => [
        "Off: speech you can already understand is heard as it is, and not read over itself.",
        "Kapalıyken: zaten anladığınız konuşma olduğu gibi duyulur, üstüne bir de okunmaz.",
        "Desactivado: lo que ya entiendes se oye tal cual y no se lee encima.",
        "Desligado: a fala que você já entende é ouvida como está, sem leitura por cima.",
        "关闭时：你本来就能听懂的语音按原样播放，不会被重复朗读。",
    ],
    ListenNote => [
        "While a voice reads, listening may pause so it is not captioned again.",
        "Bir ses okurken, okunan metin yeniden altyazıya dönüşmesin diye dinleme duraklayabilir.",
        "Mientras una voz lee, la escucha puede pausarse para no subtitularla de nuevo.",
        "Enquanto uma voz lê, a escuta pode pausar para que ela não seja legendada de novo.",
        "朗读时可能会暂停收听，以免朗读内容又被转成字幕。",
    ],
    PileUpNote => [
        "When captions pile up, they are read together and a little faster. One left more than 10 seconds behind is skipped, and so is a line read moments ago.",
        "Altyazılar birikince birlikte ve biraz daha hızlı okunur. 10 saniyeden fazla geride kalan atlanır, az önce okunan bir satır da.",
        "Cuando se acumulan subtítulos, se leen juntos y un poco más rápido. Se omite el que queda más de 10 segundos atrás, y también una línea leída hace un momento.",
        "Quando as legendas se acumulam, são lidas juntas e um pouco mais rápido. Uma que fica mais de 10 segundos para trás é pulada, assim como uma fala lida há pouco.",
        "字幕堆积时，会合在一起稍快地朗读。落后超过 10 秒的字幕会被跳过，刚读过的句子也不会重读。",
    ],
    PersonalUseAgree => ["I will use it for personal, non-commercial use only", "Yalnızca kişisel ve ticari olmayan amaçlarla kullanacağım", "Lo usaré solo para uso personal y no comercial", "Vou usá-lo apenas para uso pessoal e não comercial", "我只会将其用于个人非商业用途"],
    ReadingInSpeakersVoice => ["Reading in the speaker's voice.", "Konuşanın sesiyle okunuyor.", "Leyendo con la voz del hablante.", "Lendo com a voz de quem fala.", "正在用说话人的声音朗读。"],
    ListeningToLearn => ["Listening for a few seconds of speech to learn the voice.", "Sesi öğrenmek için birkaç saniyelik konuşma dinleniyor.", "Escuchando unos segundos de voz para aprenderla.", "Ouvindo alguns segundos de fala para aprender a voz.", "正在收听几秒钟的讲话来学习声音。"],
    LearnAgain => ["Learn again", "Yeniden öğren", "Aprender de nuevo", "Aprender de novo", "重新学习"],
    LicenceBefore => [
        "OmniVoice by k2-fsa, CC-BY-NC-4.0: for personal, non-commercial use only (",
        "k2-fsa'nın OmniVoice modeli, CC-BY-NC-4.0: yalnızca kişisel ve ticari olmayan kullanım için (",
        "OmniVoice de k2-fsa, CC-BY-NC-4.0: solo para uso personal y no comercial (",
        "OmniVoice da k2-fsa, CC-BY-NC-4.0: apenas para uso pessoal e não comercial (",
        "OmniVoice（k2-fsa），CC-BY-NC-4.0：仅限个人非商业用途（",
    ],
    Licence => ["license", "lisans", "licencia", "licença", "许可"],
    LicenceAfter => ["). Do not use it to pass yourself off as someone.", "). Kendinizi başkası gibi göstermek için kullanmayın.", "). No lo uses para hacerte pasar por otra persona.", "). Não o use para se passar por outra pessoa.", "）。请勿用它冒充他人。"],
    Automatic => ["Automatic", "Otomatik", "Automática", "Automática", "自动"],
    AutomaticHint => ["The system voice for each caption's language.", "Her altyazının dili için sistem sesi.", "La voz del sistema para el idioma de cada subtítulo.", "A voz do sistema para o idioma de cada legenda.", "按每条字幕的语言使用系统语音。"],
    Stop => ["Stop", "Durdur", "Detener", "Parar", "停止"],
    PlaySample => ["Play sample", "Örneği dinle", "Escuchar muestra", "Ouvir amostra", "试听"],
    PersonaLale => ["Clear and natural", "Net ve doğal", "Clara y natural", "Clara e natural", "清晰自然"],
    PersonaAyla => ["Cheerful and warm", "Neşeli ve sıcak", "Alegre y cálida", "Alegre e acolhedora", "欢快温暖"],
    PersonaDeniz => ["Calm and steady", "Sakin ve dengeli", "Tranquilo y sereno", "Calmo e firme", "沉稳平静"],
    PersonaAtes => ["Deep and reassuring", "Tok ve güven veren", "Grave y tranquilizador", "Grave e reconfortante", "低沉安心"],
    PersonaBilge => ["Slow and patient, easy to follow", "Yavaş ve sabırlı, takibi kolay", "Lenta y paciente, fácil de seguir", "Lenta e paciente, fácil de acompanhar", "缓慢耐心，容易跟上"],
    PersonaRuzgar => ["Fast, keeps up with quick talkers", "Hızlı, çabuk konuşanlara yetişir", "Veloz, sigue a quien habla deprisa", "Veloz, acompanha quem fala rápido", "快速，跟得上语速快的人"],
    PersonaCustom => ["Your own voice and speed", "Kendi seçtiğiniz ses ve hız", "Tu propia voz y velocidad", "Sua própria voz e velocidade", "自选声音和语速"],

    // Captions page
    Position => ["Position", "Konum", "Posición", "Posição", "位置"],
    PositionNote => [
        "Click and hold any caption to drag it. A sample is on screen while this page is open.",
        "Bir altyazıyı sürüklemek için üzerine tıklayıp basılı tutun. Bu sayfa açıkken ekranda bir örnek görünür.",
        "Mantén pulsado cualquier subtítulo para arrastrarlo. Mientras esta página está abierta se muestra un ejemplo.",
        "Clique e segure qualquer legenda para arrastá-la. Um exemplo aparece na tela enquanto esta página está aberta.",
        "按住任意字幕即可拖动。此页面打开时屏幕上会显示示例。",
    ],
    ResetPosition => ["Reset position", "Konumu sıfırla", "Restablecer posición", "Redefinir posição", "重置位置"],
    LockInPlace => ["Lock in place", "Yerine kilitle", "Fijar en su sitio", "Travar no lugar", "锁定位置"],
    LockHint => [
        "Captions cannot be dragged, and a click on one goes to the app under it.",
        "Altyazılar sürüklenemez; üzerlerine tıklayınca tıklama alttaki uygulamaya gider.",
        "Los subtítulos no se pueden arrastrar y un clic sobre ellos va a la app de debajo.",
        "As legendas não podem ser arrastadas, e um clique nelas vai para o app embaixo.",
        "字幕无法拖动，点击字幕会传给其下方的应用。",
    ],
    Streaming => ["Streaming", "Yayın", "Streaming", "Transmissão", "直播"],
    CaptionsAsWebPage => ["Captions as a web page", "Altyazıları web sayfası olarak sun", "Subtítulos como página web", "Legendas como página da web", "以网页提供字幕"],
    WebPageHint => [
        "For a stream: the caption window is not part of a captured game.",
        "Yayın için: altyazı penceresi, yakalanan oyun görüntüsünde yer almaz.",
        "Para streams: la ventana de subtítulos no forma parte del juego capturado.",
        "Para transmissões: a janela de legendas não faz parte do jogo capturado.",
        "用于直播：字幕窗口不在捕获的游戏画面中。",
    ],
    ObsNote => [
        "In OBS, add a Browser source as wide and high as your stream, with this address:",
        "OBS'te, yayınınızla aynı genişlik ve yükseklikte bir Tarayıcı kaynağı ekleyin ve şu adresi girin:",
        "En OBS, añade una fuente de Navegador del mismo ancho y alto que tu stream, con esta dirección:",
        "No OBS, adicione uma fonte Navegador com a mesma largura e altura da sua transmissão, com este endereço:",
        "在 OBS 中添加一个与直播画面同宽同高的“浏览器”来源，地址为：",
    ],
    Copy => ["Copy", "Kopyala", "Copiar", "Copiar", "复制"],
    Copied => ["Copied", "Kopyalandı", "Copiado", "Copiado", "已复制"],
    ScaleNote => ["Add ?scale=1.5 to the address for larger text.", "Daha büyük yazı için adresin sonuna ?scale=1.5 ekleyin.", "Añade ?scale=1.5 a la dirección para un texto más grande.", "Adicione ?scale=1.5 ao endereço para um texto maior.", "在地址后加上 ?scale=1.5 可放大文字。"],
    Look => ["Look", "Görünüm", "Aspecto", "Aparência", "外观"],
    TranslatedText => ["Translated text", "Çeviri metni", "Texto traducido", "Texto traduzido", "译文"],
    OriginalText => ["Original text", "Özgün metin", "Texto original", "Texto original", "原文"],
    Background => ["Background", "Arka plan", "Fondo", "Fundo", "背景"],
    CaptionsAtOnce => ["Captions at once", "Aynı anda altyazı sayısı", "Subtítulos a la vez", "Legendas ao mesmo tempo", "同时显示字幕数"],
    KeepOnScreen => ["Keep on screen", "Ekranda kalma süresi", "Tiempo en pantalla", "Tempo na tela", "停留时间"],
    TextEdge => ["Text edge", "Yazı kenarı", "Borde del texto", "Borda do texto", "文字边缘"],
    EdgeNone => ["None", "Yok", "Ninguno", "Nenhuma", "无"],
    Shadow => ["Shadow", "Gölge", "Sombra", "Sombra", "阴影"],
    Outline => ["Outline", "Kontur", "Contorno", "Contorno", "描边"],
    ShowOriginal => ["Show original text", "Özgün metni göster", "Mostrar texto original", "Mostrar texto original", "显示原文"],
    OriginalFirst => ["Original above the translation", "Özgün metin çevirinin üstünde", "Original encima de la traducción", "Original acima da tradução", "原文在译文上方"],
    CoverLookNote => [
        "While the game's subtitles are covered, only the translation shows, in a box over the subtitle: its size, colour and edge, the background, how long it stays, and where the box stands and how large it is (below) apply. The greyed settings are not used.",
        "Oyunun altyazısı kapatılırken yalnızca çeviri, altyazının üstünü kapatan bir kutuda gösterilir: çevirinin boyutu, rengi ve kenarı, arka plan, ekranda kalma süresi ve kutunun yeri ile boyutu (aşağıda) geçerlidir. Gri ayarlar kullanılmaz.",
        "Mientras se tapan los subtítulos del juego solo se ve la traducción, en un cuadro sobre el subtítulo: se aplican su tamaño, color y borde, el fondo, cuánto permanece y la posición y el tamaño del cuadro (abajo). Los ajustes en gris no se usan.",
        "Enquanto as legendas do jogo estão cobertas, só a tradução aparece, numa caixa sobre a legenda: valem o tamanho, a cor e a borda dela, o fundo, quanto tempo fica e a posição e o tamanho da caixa (abaixo). As opções em cinza não são usadas.",
        "遮住游戏字幕时只显示译文，放在盖住字幕的框里：其字号、颜色、描边、背景、停留时间以及框的位置和大小（见下方）有效。灰色的设置不起作用。",
    ],
    CoverSeeThrough => [
        "Below 100% the game's subtitle shows through the box.",
        "%100'ün altında oyunun altyazısı kutunun içinden görünür.",
        "Por debajo del 100 % el subtítulo del juego se ve a través del cuadro.",
        "Abaixo de 100% a legenda do jogo aparece através da caixa.",
        "低于 100% 时，游戏字幕会透过框显示出来。",
    ],
    CoverRight => ["Move the box right", "Kutuyu sağa kaydır", "Mover el cuadro a la derecha", "Mover a caixa para a direita", "框向右移"],
    CoverUp => ["Move the box up", "Kutuyu yukarı kaydır", "Mover el cuadro hacia arriba", "Mover a caixa para cima", "框向上移"],
    CoverPadding => ["Room around the subtitle", "Altyazı çevresindeki boşluk", "Margen alrededor del subtítulo", "Margem em volta da legenda", "字幕周围的留白"],
    CoverMaxWidth => ["Maximum box width", "Kutunun en fazla genişliği", "Ancho máximo del cuadro", "Largura máxima da caixa", "框的最大宽度"],
    ResetLook => ["Reset look", "Görünümü sıfırla", "Restablecer aspecto", "Redefinir aparência", "重置外观"],

    // Status lines
    WebPageFailed => ["The captions page could not start: {0}", "Altyazı sayfası başlatılamadı: {0}", "No se pudo iniciar la página de subtítulos: {0}", "Não foi possível iniciar a página de legendas: {0}", "无法启动字幕网页：{0}"],
    AutostartFailed => ["Could not change start at login: {0}", "Oturum açılınca başlatma ayarı değiştirilemedi: {0}", "No se pudo cambiar el inicio al iniciar sesión: {0}", "Não foi possível alterar o início ao entrar: {0}", "无法更改登录时启动：{0}"],
    InternalErrorStopped => [
        "Captions stopped after an internal error. See the log for details.",
        "Altyazılar dahili bir hata yüzünden durdu. Ayrıntılar için günlüğe bakın.",
        "Los subtítulos se detuvieron por un error interno. Consulta el registro para ver los detalles.",
        "As legendas pararam após um erro interno. Veja o registro para mais detalhes.",
        "字幕因内部错误已停止。详情请查看日志。",
    ],
    InternalErrorRestarted => ["Captions hit an internal error and restarted.", "Altyazılarda dahili bir hata oluştu ve yeniden başlatıldı.", "Los subtítulos tuvieron un error interno y se reiniciaron.", "As legendas tiveram um erro interno e reiniciaram.", "字幕遇到内部错误，已重新启动。"],
    Downloading => ["Downloading: {0}", "İndiriliyor: {0}", "Descargando: {0}", "Baixando: {0}", "正在下载：{0}"],
    PurposeTranslation => ["translation model", "çeviri modeli", "modelo de traducción", "modelo de tradução", "翻译模型"],
    PurposeSpeech => ["speech model", "konuşma modeli", "modelo de voz", "modelo de fala", "语音识别模型"],
    PurposeVoices => ["voices", "sesler", "voces", "vozes", "朗读声音"],
    LoadingTranslation => ["Loading the translation model\u{2026}", "Çeviri modeli yükleniyor\u{2026}", "Cargando el modelo de traducción\u{2026}", "Carregando o modelo de tradução\u{2026}", "正在加载翻译模型\u{2026}"],
    LoadingSpeech => ["Loading the speech model\u{2026}", "Konuşma modeli yükleniyor\u{2026}", "Cargando el modelo de voz\u{2026}", "Carregando o modelo de fala\u{2026}", "正在加载语音识别模型\u{2026}"],
    LoadingVoices => ["Loading the voices\u{2026}", "Sesler yükleniyor\u{2026}", "Cargando las voces\u{2026}", "Carregando as vozes\u{2026}", "正在加载朗读声音\u{2026}"],
    UpdateAvailableNote => [
        "Hammurabi v{0} is available. Open the tray menu to get it.",
        "Hammurabi v{0} çıktı. Almak için tepsi menüsünü açın.",
        "Hammurabi v{0} está disponible. Abre el menú de la bandeja para obtenerlo.",
        "O Hammurabi v{0} está disponível. Abra o menu da bandeja para obtê-lo.",
        "Hammurabi v{0} 已发布。打开托盘菜单即可获取。",
    ],
    ModelStillOpen => [
        "Hammurabi still has that model open. Quit it from the tray icon, open it again and delete the model before starting captions.",
        "Hammurabi bu modeli hâlâ açık tutuyor. Tepsi simgesinden Hammurabi'yi kapatın, yeniden açın ve altyazıları başlatmadan önce modeli silin.",
        "Hammurabi aún tiene ese modelo abierto. Ciérralo desde el icono de la bandeja, vuelve a abrirlo y elimina el modelo antes de iniciar los subtítulos.",
        "O Hammurabi ainda está com esse modelo aberto. Feche-o pelo ícone da bandeja, abra de novo e exclua o modelo antes de iniciar as legendas.",
        "Hammurabi 仍在使用该模型。请从托盘图标退出，重新打开，并在开始字幕前删除该模型。",
    ],
    Idle => ["Idle", "Beklemede", "En espera", "Parado", "空闲"],
    Loading => ["Loading\u{2026}", "Yükleniyor\u{2026}", "Cargando\u{2026}", "Carregando\u{2026}", "正在加载\u{2026}"],
    CaptionsOnShort => ["Captions on", "Altyazılar açık", "Subtítulos activados", "Legendas ligadas", "字幕已开启"],
    HotkeysUnavailable => ["Global hotkeys unavailable: {0}", "Genel kısayollar kullanılamıyor: {0}", "Atajos globales no disponibles: {0}", "Atalhos globais indisponíveis: {0}", "全局快捷键不可用：{0}"],
    ShortcutTaken => [
        "That shortcut is taken by another app. Pick another in Settings > General.",
        "Bu kısayolu başka bir uygulama kullanıyor. Ayarlar > Genel bölümünden başka bir kısayol seçin.",
        "Otra app ya usa ese atajo. Elige otro en Ajustes > General.",
        "Outro app já usa esse atalho. Escolha outro em Configurações > Geral.",
        "该快捷键已被其他应用占用。请在“设置 > 常规”中另选一个。",
    ],

    // Tray menu
    Translate => ["Translate", "Çevir", "Traducir", "Traduzir", "翻译"],
    StartAtLogin => ["Start at login", "Oturum açılınca başlat", "Abrir al iniciar sesión", "Abrir ao entrar", "登录时启动"],
    SourceX => ["Source: {0}", "Kaynak: {0}", "Fuente: {0}", "Fonte: {0}", "来源：{0}"],
    TranslateToX => ["Translate to: {0}", "Hedef dil: {0}", "Traducir a: {0}", "Traduzir para: {0}", "翻译为：{0}"],
    VoiceX => ["Voice: {0}", "Ses: {0}", "Voz: {0}", "Voz: {0}", "声音：{0}"],
    LearnVoiceAgain => ["Learn the voice again", "Sesi yeniden öğren", "Aprender la voz de nuevo", "Aprender a voz de novo", "重新学习声音"],
    SettingsMenu => ["Settings\u{2026}", "Ayarlar\u{2026}", "Ajustes\u{2026}", "Configurações\u{2026}", "设置\u{2026}"],
    AboutX => ["About Hammurabi v{0}", "Hammurabi v{0} hakkında", "Acerca de Hammurabi v{0}", "Sobre o Hammurabi v{0}", "关于 Hammurabi v{0}"],
    Quit => ["Quit", "Çıkış", "Salir", "Sair", "退出"],
    UpdateAvailableX => ["Update available: v{0}", "Güncelleme var: v{0}", "Actualización disponible: v{0}", "Atualização disponível: v{0}", "有可用更新：v{0}"],

    // From the engine
    CantKeepUp => ["Your computer can't keep up, choose a smaller model", "Bilgisayarınız yetişemiyor, daha küçük bir model seçin", "Tu equipo no da abasto, elige un modelo más pequeño", "Seu computador não está dando conta, escolha um modelo menor", "电脑跟不上，请选择较小的模型"],
    AudioCaptureFailed => ["Audio capture failed: {0}", "Ses yakalanamadı: {0}", "Falló la captura de audio: {0}", "Falha na captura de áudio: {0}", "音频捕获失败：{0}"],
    AudioDeviceChanged => ["Audio device changed, captions continue", "Ses cihazı değişti, altyazılar devam ediyor", "Cambió el dispositivo de audio, los subtítulos continúan", "O dispositivo de áudio mudou, as legendas continuam", "音频设备已更改，字幕继续"],
    AudioDeviceLost => ["Audio device lost, waiting for it", "Ses cihazı bağlantısı kesildi, bekleniyor", "Se perdió el dispositivo de audio, esperándolo", "Dispositivo de áudio perdido, aguardando", "音频设备已断开，正在等待"],
    LargeTooSlow => ["Large speech model is too slow here, using Turbo", "Large konuşma modeli bu bilgisayarda fazla yavaş, Turbo kullanılıyor", "El modelo de voz Large es demasiado lento aquí, se usa Turbo", "O modelo de fala Large é lento demais aqui, usando o Turbo", "Large 语音识别模型在此太慢，改用 Turbo"],
    SpeechStoppedAnswering => ["Speech model stopped answering, reloading", "Konuşma modeli yanıt vermiyor, yeniden yükleniyor", "El modelo de voz dejó de responder, recargando", "O modelo de fala parou de responder, recarregando", "语音识别模型无响应，正在重新加载"],
    SpeechKeepsFailing => ["Speech model keeps failing, reloading", "Konuşma modeli sürekli hata veriyor, yeniden yükleniyor", "El modelo de voz sigue fallando, recargando", "O modelo de fala continua falhando, recarregando", "语音识别模型反复出错，正在重新加载"],
    DownloadPaused => ["Download paused, it continues on the next Start", "İndirme duraklatıldı, bir sonraki başlatmada devam edecek", "Descarga en pausa, continuará al volver a iniciar", "Download pausado, continua no próximo início", "下载已暂停，下次开始时继续"],
    ListeningWith => ["Listening with {0}", "{0} ile dinleniyor", "Escuchando con {0}", "Ouvindo com {0}", "正在用 {0} 收听"],
    SpeechFailedRetry => ["Speech model failed, trying again", "Konuşma modeli hata verdi, yeniden deneniyor", "Falló el modelo de voz, reintentando", "O modelo de fala falhou, tentando de novo", "语音识别模型出错，正在重试"],
    SpeechFailed => ["Speech model failed: {0}", "Konuşma modeli hata verdi: {0}", "Falló el modelo de voz: {0}", "O modelo de fala falhou: {0}", "语音识别模型出错：{0}"],
    ScreenWindowsOnly => [
        "Subtitles can only be read off the screen on Windows, listening instead",
        "Altyazılar yalnızca Windows'ta ekrandan okunabilir, bunun yerine dinleniyor",
        "Los subtítulos solo se pueden leer de la pantalla en Windows, se escucha en su lugar",
        "As legendas só podem ser lidas da tela no Windows, ouvindo em vez disso",
        "只有 Windows 能从屏幕读取字幕，改为收听",
    ],
    TheMainDisplay => ["the main display", "ana ekran", "la pantalla principal", "a tela principal", "主显示器"],
    RunningScreen => ["Running: reading subtitles from {0}", "Çalışıyor: altyazılar {0} üzerinden okunuyor", "En marcha: leyendo subtítulos de {0}", "Em execução: a fonte das legendas é {0}", "运行中：正在从{0}读取字幕"],
    WaitingForWindow => ["Waiting for the window {0} to read subtitles from", "Altyazıların okunacağı {0} penceresi bekleniyor", "Esperando la ventana {0} para leer sus subtítulos", "Aguardando a janela {0} para ler as legendas", "正在等待窗口 {0} 以读取字幕"],
    WaitingFor => ["Waiting for {0} to read subtitles from", "Altyazıların okunacağı {0} bekleniyor", "Esperando {0} para leer sus subtítulos", "Aguardando {0} para ler as legendas", "正在等待{0}以读取字幕"],
    WindowMinimized => [
        "The window {0} is minimized: bring it back to have its subtitles read",
        "{0} penceresi simge durumuna küçültülmüş: altyazılarının okunması için geri getirin",
        "La ventana {0} está minimizada: restáurala para leer sus subtítulos",
        "A janela {0} está minimizada: restaure-a para que as legendas sejam lidas",
        "窗口 {0} 已最小化：请将其恢复以读取字幕",
    ],
    ScreenFailed => ["Cannot read subtitles off the screen: {0}", "Altyazılar ekrandan okunamıyor: {0}", "No se pueden leer los subtítulos de la pantalla: {0}", "Não é possível ler as legendas da tela: {0}", "无法从屏幕读取字幕：{0}"],
    LiveCaptionsUnsupported => [
        "Windows Live Captions is not supported on this system, using Whisper",
        "Windows canlı açıklamalı alt yazıları bu sistemde desteklenmiyor, Whisper kullanılıyor",
        "Subtítulos en directo de Windows no es compatible con este sistema, se usa Whisper",
        "Legendas ao Vivo do Windows não é compatível com este sistema, usando o Whisper",
        "此系统不支持 Windows 实时字幕，改用 Whisper",
    ],
    RunningLiveCaptions => ["Running: Windows Live Captions", "Çalışıyor: Windows canlı açıklamalı alt yazıları", "En marcha: Subtítulos en directo de Windows", "Em execução: Legendas ao Vivo do Windows", "运行中：Windows 实时字幕"],
    LiveCaptionsUnavailable => [
        "Windows Live Captions unavailable ({0}), using Whisper",
        "Windows canlı açıklamalı alt yazıları kullanılamıyor ({0}), Whisper kullanılıyor",
        "Subtítulos en directo de Windows no disponible ({0}), se usa Whisper",
        "Legendas ao Vivo do Windows indisponível ({0}), usando o Whisper",
        "Windows 实时字幕不可用（{0}），改用 Whisper",
    ],
    TranslationReady => ["Translation ready", "Çeviri hazır", "Traducción lista", "Tradução pronta", "翻译已就绪"],
    NaturalTooSlow => [
        "The Natural translator is too slow here. It needs a graphics card with 8 GB free. Try its smaller model, or Fast.",
        "Doğal çevirici bu bilgisayarda fazla yavaş. 8 GB boş belleği olan bir ekran kartı ister. Küçük modelini ya da Hızlı'yı deneyin.",
        "El traductor Natural es demasiado lento aquí. Necesita una tarjeta gráfica con 8 GB libres. Prueba su modelo más pequeño o Rápido.",
        "O tradutor Natural é lento demais aqui. Ele precisa de uma placa de vídeo com 8 GB livres. Experimente o modelo menor ou o Rápido.",
        "自然翻译器在此太慢。它需要有 8 GB 可用显存的显卡。请试试它的较小模型或“快速”。",
    ],
    NaturalLightTooSlow => [
        "The Natural translator is too slow here. Its smaller model needs a graphics card with 3.5 GB free. Try Fast.",
        "Doğal çevirici bu bilgisayarda fazla yavaş. Küçük modeli 3,5 GB boş belleği olan bir ekran kartı ister. Hızlı'yı deneyin.",
        "El traductor Natural es demasiado lento aquí. Su modelo más pequeño necesita una tarjeta gráfica con 3,5 GB libres. Prueba Rápido.",
        "O tradutor Natural é lento demais aqui. O modelo menor precisa de uma placa de vídeo com 3,5 GB livres. Experimente o Rápido.",
        "自然翻译器在此太慢。它的较小模型需要有 3.5 GB 可用显存的显卡。请试试“快速”。",
    ],
    TranslationFailingRetry => ["Translation is failing, trying again: {0}", "Çeviri hata veriyor, yeniden deneniyor: {0}", "La traducción está fallando, reintentando: {0}", "A tradução está falhando, tentando de novo: {0}", "翻译出错，正在重试：{0}"],
    TranslationFailedRetry => ["Translation failed, trying again", "Çeviri hata verdi, yeniden deneniyor", "Falló la traducción, reintentando", "A tradução falhou, tentando de novo", "翻译失败，正在重试"],
    TranslationUnavailable => ["Translation unavailable: {0}", "Çeviri kullanılamıyor: {0}", "Traducción no disponible: {0}", "Tradução indisponível: {0}", "翻译不可用：{0}"],
    DownloadFinished => ["Download finished", "İndirme tamamlandı", "Descarga completada", "Download concluído", "下载完成"],
    DownloadFailed => ["Download failed: {0}", "İndirme başarısız oldu: {0}", "Falló la descarga: {0}", "Falha no download: {0}", "下载失败：{0}"],
    SelectedOutputMissing => ["The selected audio output is not available, using the default", "Seçilen ses çıkışı kullanılamıyor, varsayılan kullanılıyor", "La salida de audio elegida no está disponible, se usa la predeterminada", "A saída de áudio escolhida não está disponível, usando a padrão", "所选音频输出不可用，改用默认设备"],
    SelectedMicMissing => ["The selected microphone is not available, using the default", "Seçilen mikrofon kullanılamıyor, varsayılan kullanılıyor", "El micrófono elegido no está disponible, se usa el predeterminado", "O microfone escolhido não está disponível, usando o padrão", "所选麦克风不可用，改用默认设备"],
    SystemAudioUsingMic => ["System audio unavailable ({0}), using the microphone", "Sistem sesi kullanılamıyor ({0}), mikrofon kullanılıyor", "Audio del sistema no disponible ({0}), se usa el micrófono", "Áudio do sistema indisponível ({0}), usando o microfone", "系统声音不可用（{0}），改用麦克风"],
    SystemAudioMicOnly => ["System audio unavailable ({0}), using the microphone only", "Sistem sesi kullanılamıyor ({0}), yalnızca mikrofon kullanılıyor", "Audio del sistema no disponible ({0}), se usa solo el micrófono", "Áudio do sistema indisponível ({0}), usando só o microfone", "系统声音不可用（{0}），仅使用麦克风"],
    MicSystemOnly => ["Microphone unavailable ({0}), using system audio only", "Mikrofon kullanılamıyor ({0}), yalnızca sistem sesi kullanılıyor", "Micrófono no disponible ({0}), se usa solo el audio del sistema", "Microfone indisponível ({0}), usando só o áudio do sistema", "麦克风不可用（{0}），仅使用系统声音"],

    // Reading aloud
    MissingVoice => ["No {0} voice is installed for reading aloud. {1}", "Sesli okuma için yüklü bir {0} sesi yok. {1}", "No hay ninguna voz instalada para leer en {0}. {1}", "Não há nenhuma voz instalada para ler em {0}. {1}", "没有安装可用于朗读的 {0} 语音。{1}"],
    NaturalVoicesHint => [
        "Or pick Natural voices in Settings > Read aloud, which read it without one.",
        "Ya da Ayarlar > Sesli okuma bölümünde, ses yüklemeden okuyan Doğal sesler'i seçin.",
        "O elige Voces naturales en Ajustes > Leer en voz alta, que lo leen sin instalar ninguna.",
        "Ou escolha Vozes naturais em Configurações > Ler em voz alta, que leem sem instalar nenhuma.",
        "也可以在 设置 > 朗读 中选择“自然语音”，无需安装语音即可朗读。",
    ],
    VoiceHintWindows => [
        "Add one in Windows Settings > Time & language > Speech > Manage voices.",
        "Windows Ayarlar > Saat ve dil > Konuşma > Sesleri yönet bölümünden ekleyebilirsiniz.",
        "Añade una en Configuración de Windows > Hora e idioma > Voz > Administrar voces.",
        "Adicione uma em Configurações do Windows > Hora e idioma > Fala > Gerenciar vozes.",
        "请在 Windows 设置 > 时间和语言 > 语音 > 管理语音 中添加。",
    ],
    VoiceHintMac => [
        "Add one in System Settings > Accessibility > Spoken Content (Read & Speak) > System voice > Manage Voices.",
        "Sistem Ayarları > Erişilebilirlik > Seslendirilen İçerik > Sistem sesi > Sesleri Yönet bölümünden ekleyebilirsiniz.",
        "Añade una en Ajustes del Sistema > Accesibilidad > Contenido leído > Voz del sistema > Gestionar voces.",
        "Adicione uma em Ajustes do Sistema > Acessibilidade > Conteúdo Falado > Voz do sistema > Gerenciar Vozes.",
        "请在 系统设置 > 辅助功能 > 朗读内容 > 系统声音 > 管理声音 中添加。",
    ],
    VoiceHintLinux => ["Install a voice for it in speech-dispatcher or espeak-ng.", "speech-dispatcher ya da espeak-ng için bu dilde bir ses yükleyin.", "Instala una voz para ese idioma en speech-dispatcher o espeak-ng.", "Instale uma voz para esse idioma no speech-dispatcher ou no espeak-ng.", "请在 speech-dispatcher 或 espeak-ng 中安装该语言的语音。"],
    SpeakerVoiceCouldNotStart => ["Speaker's voice could not start, reading with other voices: {0}", "Konuşanın sesi başlatılamadı, diğer seslerle okunuyor: {0}", "La voz del hablante no pudo iniciarse, se lee con otras voces: {0}", "A voz de quem fala não pôde iniciar, lendo com outras vozes: {0}", "说话人的声音无法启动，改用其他声音朗读：{0}"],
    SpeakerVoiceCouldNotRead => ["Speaker's voice could not read aloud, using other voices: {0}", "Konuşanın sesiyle okunamadı, diğer sesler kullanılıyor: {0}", "La voz del hablante no pudo leer, se usan otras voces: {0}", "A voz de quem fala não conseguiu ler, usando outras vozes: {0}", "无法用说话人的声音朗读，改用其他声音：{0}"],
    SpeakerVoiceNotDownloaded => [
        "Speaker's voice is not downloaded yet (Settings > Read aloud), using other voices",
        "Konuşanın sesi henüz indirilmedi (Ayarlar > Sesli okuma), diğer sesler kullanılıyor",
        "La voz del hablante aún no se ha descargado (Ajustes > Leer en voz alta), se usan otras voces",
        "A voz de quem fala ainda não foi baixada (Configurações > Ler em voz alta), usando outras vozes",
        "说话人的声音尚未下载（设置 > 朗读），改用其他声音",
    ],
    SupertonicFailed => ["Supertonic could not read aloud, using a system voice: {0}", "Supertonic ile okunamadı, sistem sesi kullanılıyor: {0}", "Supertonic no pudo leer, se usa una voz del sistema: {0}", "O Supertonic não conseguiu ler, usando uma voz do sistema: {0}", "Supertonic 无法朗读，改用系统语音：{0}"],
    ReadingFailed => ["Reading aloud failed: {0}", "Sesli okuma başarısız oldu: {0}", "Falló la lectura en voz alta: {0}", "Falha na leitura em voz alta: {0}", "朗读失败：{0}"],
    TheSpeakersVoice => ["the speaker's voice", "konuşanın sesi", "la voz del hablante", "a voz de quem fala", "说话人的声音"],
    OmniVoiceOwnVoice => ["OmniVoice's own voice", "OmniVoice'un kendi sesi", "la voz propia de OmniVoice", "a voz própria do OmniVoice", "OmniVoice 自带的声音"],
    ASystemVoice => ["a system voice", "bir sistem sesi", "una voz del sistema", "uma voz do sistema", "系统语音"],

    // Language names
    LangEn => ["English", "İngilizce", "Inglés", "Inglês", "英语"],
    LangEs => ["Spanish", "İspanyolca", "Español", "Espanhol", "西班牙语"],
    LangFr => ["French", "Fransızca", "Francés", "Francês", "法语"],
    LangDe => ["German", "Almanca", "Alemán", "Alemão", "德语"],
    LangIt => ["Italian", "İtalyanca", "Italiano", "Italiano", "意大利语"],
    LangPt => ["Portuguese", "Portekizce", "Portugués", "Português", "葡萄牙语"],
    LangNl => ["Dutch", "Felemenkçe", "Neerlandés", "Holandês", "荷兰语"],
    LangPl => ["Polish", "Lehçe", "Polaco", "Polonês", "波兰语"],
    LangRu => ["Russian", "Rusça", "Ruso", "Russo", "俄语"],
    LangUk => ["Ukrainian", "Ukraynaca", "Ucraniano", "Ucraniano", "乌克兰语"],
    LangTr => ["Turkish", "Türkçe", "Turco", "Turco", "土耳其语"],
    LangAr => ["Arabic", "Arapça", "Árabe", "Árabe", "阿拉伯语"],
    LangFa => ["Persian", "Farsça", "Persa", "Persa", "波斯语"],
    LangHi => ["Hindi", "Hintçe", "Hindi", "Hindi", "印地语"],
    LangBn => ["Bengali", "Bengalce", "Bengalí", "Bengali", "孟加拉语"],
    LangUr => ["Urdu", "Urduca", "Urdu", "Urdu", "乌尔都语"],
    LangId => ["Indonesian", "Endonezce", "Indonesio", "Indonésio", "印度尼西亚语"],
    LangVi => ["Vietnamese", "Vietnamca", "Vietnamita", "Vietnamita", "越南语"],
    LangTh => ["Thai", "Tayca", "Tailandés", "Tailandês", "泰语"],
    LangJa => ["Japanese", "Japonca", "Japonés", "Japonês", "日语"],
    LangKo => ["Korean", "Korece", "Coreano", "Coreano", "韩语"],
    LangZh => ["Chinese", "Çince", "Chino", "Chinês", "中文"],
    LangZhHant => ["Chinese (Traditional)", "Çince (Geleneksel)", "Chino (tradicional)", "Chinês (tradicional)", "中文（繁体）"],
    LangEl => ["Greek", "Yunanca", "Griego", "Grego", "希腊语"],
    LangSv => ["Swedish", "İsveççe", "Sueco", "Sueco", "瑞典语"],
    LangRo => ["Romanian", "Romence", "Rumano", "Romeno", "罗马尼亚语"],
    LangCs => ["Czech", "Çekçe", "Checo", "Tcheco", "捷克语"],
    LangHu => ["Hungarian", "Macarca", "Húngaro", "Húngaro", "匈牙利语"],
    LangHe => ["Hebrew", "İbranice", "Hebreo", "Hebraico", "希伯来语"],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<usize> {
        let mut found: Vec<usize> = (0..10)
            .filter(|i| s.contains(&format!("{{{i}}}")))
            .collect();
        found.sort_unstable();
        found
    }

    #[test]
    fn every_text_is_in_every_language() {
        for &key in T::ALL {
            let texts = key.texts();
            for (lang, text) in LANGS.iter().zip(texts) {
                assert!(!text.trim().is_empty(), "{key:?} has no {lang:?} text");
                assert_eq!(
                    placeholders(text),
                    placeholders(texts[0]),
                    "{key:?} in {lang:?} takes other arguments than in English"
                );
                assert!(
                    !text.contains('\u{2014}'),
                    "{key:?} in {lang:?} has an em dash"
                );
            }
        }
    }

    #[test]
    fn every_language_has_a_name() {
        use crate::settings::{SOURCE_LANGS, TARGET_LANGS};
        for c in SOURCE_LANGS.iter().chain(TARGET_LANGS.iter()) {
            assert_ne!(language_name(c), *c);
        }
        assert_eq!(language_name("xx"), "xx");
    }

    #[test]
    fn missing_texts_fall_back_to_english() {
        assert_eq!(text_in(T::Quit, Lang::Tr), "Çıkış");
        assert_eq!(text_in(T::Quit, Lang::En), "Quit");
    }

    #[test]
    fn language_from_locale() {
        assert_eq!(from_locale("tr-TR"), Lang::Tr);
        assert_eq!(from_locale("tr"), Lang::Tr);
        assert_eq!(from_locale("TR_tr.UTF-8"), Lang::Tr);
        assert_eq!(from_locale("pt-BR"), Lang::Pt);
        assert_eq!(from_locale("pt-PT"), Lang::Pt);
        assert_eq!(from_locale("es-MX"), Lang::Es);
        assert_eq!(from_locale("zh-CN"), Lang::Zh);
        assert_eq!(from_locale("zh-Hans-CN"), Lang::Zh);
        assert_eq!(from_locale("zh-TW"), Lang::En);
        assert_eq!(from_locale("zh-Hant-HK"), Lang::En);
        assert_eq!(from_locale("en-US"), Lang::En);
        assert_eq!(from_locale("de-DE"), Lang::En);
        assert_eq!(from_locale(""), Lang::En);
        assert_eq!(from_locale("garbage"), Lang::En);
    }

    #[test]
    fn arguments_go_in_their_places() {
        CURRENT.store(Lang::En as u8, Ordering::Relaxed);
        assert_eq!(
            tf(T::KeepUpNo, &[&"1.2 s", &t(T::VerbHear), &"Try Tiny."]),
            "No: a line takes 1.2 s to hear. Try Tiny."
        );
        assert_eq!(decimal(1.25, 2), "1.25");
    }
}
