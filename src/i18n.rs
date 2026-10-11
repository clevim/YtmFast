//! Interface language: Brazilian Portuguese (the texts in the code) or English. `tr` looks a text
//! up when English is on; `tf!` does the same for texts with values (`{}` placeholders).
//! Anything without a translation shows as written.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// "auto" follows the system (Portuguese if LANG/LC_* says pt, English otherwise), "pt", "en".
pub fn apply(setting: &str) {
    let en = match setting {
        "pt" => false,
        "en" => true,
        _ => !system_is_portuguese(),
    };
    ENGLISH.store(en, Ordering::Relaxed);
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

fn system_is_portuguese() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("pt"))
}

/// `{}` placeholders filled in order (the template comes from `tr`, so it can't be a format! literal).
pub fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = args.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// `format!` for interface texts: translated template, `{}` only (format values before passing).
#[macro_export]
macro_rules! tf {
    ($t:expr $(, $a:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($t), &[$(&$a as &dyn std::fmt::Display),*])
    };
}

/// The English text for `s`, or `s` itself (Portuguese on, or no translation).
pub fn tr(s: &str) -> &str {
    if !english() {
        return s;
    }
    en(s).unwrap_or(s)
}

fn en(s: &str) -> Option<&'static str> {
    Some(match s {
        // Navigation and pages
        "Início" => "Home",
        "Fila" => "Queue",
        "FILA" => "QUEUE",
        "Letra" => "Lyrics",
        "Entrar" => "Sign in",
        "Sair" => "Quit",
        "Biblioteca" => "Library",
        "Buscar" => "Search",
        "Pesquisar" => "Search",
        "Pesquisar músicas, artistas, álbuns e playlists" => "Search songs, artists, albums and playlists",
        "Configurações" => "Settings",
        "Configurações (Ctrl+,)" => "Settings (Ctrl+,)",
        "Vídeo" => "Video",
        "Vídeo (Ctrl+Shift+K)" => "Video (Ctrl+Shift+K)",
        "Mini player" => "Mini player",
        "Mini player (Ctrl+M)" => "Mini player (Ctrl+M)",
        "Músicas" => "Songs",
        "Músicas curtidas" => "Liked songs",
        "Músicas de {}" => "Songs by {}",
        "Ver todas as músicas" => "See all songs",
        "Artista" => "Artist",
        "Artistas" => "Artists",
        "Artistas semelhantes" => "Similar artists",
        "Álbum" => "Album",
        "Álbum · {}" => "Album · {}",
        "Álbuns" => "Albums",
        "Álbuns e singles" => "Albums and singles",
        "Playlist" => "Playlist",
        "Playlists" => "Playlists",
        "Playlist automática" => "Auto playlist",
        "Playlist automática · {} faixas" => "Auto playlist · {} songs",
        "{} · {} faixas" => "{} · {} songs",
        "{} faixas • {}" => "{} songs • {}",
        "{} faixas{}" => "{} songs{}",
        "{} faixas" => "{} songs",
        "{} faixa" => "{} song",
        "Suas playlists" => "Your playlists",
        "Seus álbuns" => "Your albums",
        "Ouvir novamente" => "Listen again",
        "Novos lançamentos" => "New releases",
        "Recentes" => "Recent",
        "A seguir" => "Up next",
        "Rádio" => "Radio",
        " • rádio" => " • radio",
        "Bom dia" => "Good morning",
        "Boa tarde" => "Good afternoon",
        "Boa noite" => "Good evening",
        "Entre para ver sua biblioteca" => "Sign in to see your library",
        "Playlists, curtidas e áudio na qualidade Premium." => "Playlists, likes and Premium-quality audio.",
        "Filtrar biblioteca" => "Filter library",
        "Filtrar por nome" => "Filter by name",
        // Player
        "Tocar" => "Play",
        "Pausar" => "Pause",
        "Parar" => "Stop",
        "Anterior" => "Previous",
        "Próxima" => "Next",
        "Aleatório" => "Shuffle",
        "Aleatório (S)" => "Shuffle (S)",
        "Repetir" => "Repeat",
        "Repetir (R)" => "Repeat (R)",
        "Fila (Q)" => "Queue (Q)",
        "Letra (L)" => "Lyrics (L)",
        "Curtir" => "Like",
        "Curtir (B)" => "Like (B)",
        "Mudo" => "Mute",
        "Volume" => "Volume",
        "Volume {}%" => "Volume {}%",
        "Volume {}% · roda do mouse ajusta · clique: mudo (M)" => "Volume {}% · mouse wheel adjusts · click: mute (M)",
        "Nada tocando" => "Nothing playing",
        "nada tocando" => "nothing playing",
        "YtmFast — nada tocando" => "YtmFast — nothing playing",
        "Tela cheia" => "Full screen",
        "Tela cheia (F)" => "Full screen (F)",
        "Sair da tela cheia (Esc)" => "Exit full screen (Esc)",
        "Voltar ao app (Ctrl+M)" => "Back to the app (Ctrl+M)",
        "Minimizar" => "Minimize",
        "Fechar" => "Close",
        "Tempo decorrido / restante" => "Elapsed / remaining time",
        "ESTÉREO" => "STEREO",
        "EQUALIZADOR" => "EQUALIZER",
        "Equalizador" => "Equalizer",
        "Personalizado" => "Custom",
        "Pré" => "Pre",
        "{}: {} dB · duplo clique zera" => "{}: {} dB · double-click resets",
        "Ligar / desligar o equalizador" => "Turn the equalizer on / off",
        "Loudness adaptativo: mais graves e agudos com volume baixo" => "Adaptive loudness: more bass and treble at low volume",
        "  A fila está vazia." => "  The queue is empty.",
        "Duplo clique toca" => "Double-click to play",
        "Tocar agora" => "Play now",
        "Tocar a seguir" => "Play next",
        "Adicionar à fila" => "Add to queue",
        "Adicionar à playlist" => "Add to playlist",
        "Remover da fila" => "Remove from queue",
        "Remover daqui" => "Remove from here",
        "Remover das curtidas" => "Remove from liked songs",
        "Iniciar rádio" => "Start radio",
        "Ir para {}" => "Go to {}",
        "Ir para o álbum" => "Go to album",
        "Ir para este trecho" => "Jump here",
        "Trecho mais repetido · clique para ir" => "Most replayed part · click to jump",
        "Mover para cima" => "Move up",
        "Mover para baixo" => "Move down",
        "Salvar como playlist" => "Save as playlist",
        "Nova playlist" => "New playlist",
        "Nome da playlist" => "Playlist name",
        "Confirmar exclusão" => "Confirm deletion",
        "Excluir" => "Delete",
        "Cancelar" => "Cancel",
        "Limpar" => "Clear",
        "Centralizar" => "Center",
        "Nada tocado ainda." => "Nothing played yet.",
        "O histórico está desligado nas configurações." => "History is turned off in settings.",
        // Lyrics and video
        "Carregando letra…" => "Loading lyrics…",
        "Toque algo para ver a letra." => "Play something to see the lyrics.",
        "Sem letra para \"{}\"." => "No lyrics for \"{}\".",
        "Fonte: LRCLIB" => "Source: LRCLIB",
        "Fonte: KuGou" => "Source: KuGou",
        "Estilo da letra" => "Lyrics style",
        "Por linha" => "Line by line",
        "Palavra por palavra (karaokê)" => "Word by word (karaoke)",
        "Toque algo para ver o vídeo." => "Play something to see the video.",
        "Carregando o vídeo…" => "Loading the video…",
        "Esta faixa é só áudio." => "This track is audio only.",
        "Procurar videoclipe" => "Find music video",
        "Procurando o videoclipe…" => "Looking for the music video…",
        "Videoclipe: {}" => "Music video: {}",
        "Não encontrei um videoclipe para esta faixa." => "No music video found for this track.",
        "Não consegui carregar o vídeo desta faixa." => "Couldn't load this track's video.",
        "Vídeo indisponível neste sistema (requer OpenGL)." => "Video isn't available on this system (needs OpenGL).",
        "Carregando artista…" => "Loading artist…",
        // Status messages
        "Montando rádio…" => "Building radio…",
        "Abrindo link…" => "Opening link…",
        "Adicionada às curtidas" => "Added to liked songs",
        "Removida das curtidas" => "Removed from liked songs",
        "\"{}\" adicionada à playlist" => "\"{}\" added to the playlist",
        "\"{}\" removida da playlist" => "\"{}\" removed from the playlist",
        "Playlist excluída" => "Playlist deleted",
        "entre na conta para curtir" => "sign in to like songs",
        "Não consegui abrir \"{}\" ({})" => "Couldn't open \"{}\" ({})",
        "Cache limpo" => "Cache cleared",
        "Você saiu da conta" => "You signed out",
        "Conectado ao YouTube Music" => "Connected to YouTube Music",
        "Conectado ao YouTube Music." => "Connected to YouTube Music.",
        "O tempo para entrar acabou. Tente de novo." => "Sign-in timed out. Please try again.",
        "O yt-dlp {} está desatualizado; atualize se a reprodução falhar (YouTube muda com frequência)." => {
            "yt-dlp {} is outdated; update it if playback fails (YouTube changes often)."
        }
        "yt-dlp não encontrado" => "yt-dlp not found",
        "Baixando a skin…" => "Downloading the skin…",
        "Skin instalada. Abra o mini player (Ctrl+M) para usar." => "Skin installed. Open the mini player (Ctrl+M) to use it.",
        "Erro baixando a skin: {}" => "Error downloading the skin: {}",
        "Erro na skin {}: {}" => "Error in skin {}: {}",
        "Erro lendo arquivo: {}" => "Error reading file: {}",
        "Erro" => "Error",
        "arquivo inválido" => "invalid file",
        "nome de skin inválido" => "invalid skin name",
        "skin grande demais" => "skin too large",
        "skin sem main.bmp" => "skin without main.bmp",
        // Sign-in
        "Entre no YtmFast" => "Sign in to YtmFast",
        "Entrar com o Google" => "Sign in with Google",
        "Use sua conta do Google para ver sua biblioteca, curtidas e playlists, e ouvir na qualidade Premium." => {
            "Use your Google account to see your library, likes and playlists, and listen in Premium quality."
        }
        "Continuar com o {}" => "Continue with {}",
        "Sua conta do YouTube já está aberta nele" => "Your YouTube account is already open there",
        "Abre o login do Google no {}." => "Opens Google sign-in in {}.",
        "Procurando sua conta no {}…" => "Looking for your account in {}…",
        "Termine o login no {}" => "Finish signing in in {}",
        "O YtmFast continua sozinho assim que você entrar." => "YtmFast carries on by itself once you sign in.",
        "Abrir o login de novo" => "Open sign-in again",
        "Login detectado no {}. Entrando…" => "Sign-in found in {}. Signing in…",
        "Entrando com o {}…" => "Signing in with {}…",
        "Nenhum navegador compatível encontrado." => "No compatible browser found.",
        "Opções avançadas" => "Advanced options",
        "Entrar com um arquivo cookies.txt exportado do navegador:" => "Sign in with a cookies.txt file exported from your browser:",
        "Sem login: busca e reprodução funcionam, biblioteca não." => "Without signing in: search and playback work, the library doesn't.",
        "Sua senha nunca passa pelo YtmFast. Ficam guardados só os cookies do YouTube, acessíveis apenas pelo seu usuário." => {
            "Your password never goes through YtmFast. Only the YouTube cookies are kept, readable by your user alone."
        }
        "Não achei um login do YouTube no {}. Entre em music.youtube.com nele e tente de novo." => {
            "No YouTube sign-in found in {}. Sign in to music.youtube.com there and try again."
        }
        "Perfil do {} não encontrado." => "{} profile not found.",
        "navegador desconhecido" => "unknown browser",
        "Trocar de conta" => "Switch account",
        "Sair da conta" => "Sign out",
        // Settings: tabs and cards
        "Reprodução" => "Playback",
        "Aparência" => "Appearance",
        "Janela" => "Window",
        "Conta" => "Account",
        "Rede" => "Network",
        "Armazenamento" => "Storage",
        "Sobre" => "About",
        "Atalhos de teclado" => "Keyboard shortcuts",
        "Atalhos de teclado (?)" => "Keyboard shortcuts (?)",
        // Settings: playback
        "Qualidade do áudio" => "Audio quality",
        "Alta" => "High",
        "Normal" => "Normal",
        "Economia de dados" => "Data saver",
        "Saída de áudio" => "Audio output",
        "Automático (padrão do sistema)" => "Automatic (system default)",
        "Sem pausa entre faixas" => "Gapless playback",
        "Emenda uma música na outra (gapless)" => "Songs run into each other without a gap",
        "Transição suave (crossfade)" => "Crossfade",
        "Desligada" => "Off",
        "2 segundos" => "2 seconds",
        "4 segundos" => "4 seconds",
        "6 segundos" => "6 seconds",
        "8 segundos" => "8 seconds",
        "10 segundos" => "10 seconds",
        "12 segundos" => "12 seconds",
        "Normalizar volume" => "Normalize volume",
        "Deixa todas as músicas com volume parecido" => "Keeps every song at a similar loudness",
        "Reprodução automática" => "Autoplay",
        "Continua com músicas parecidas quando a fila acaba" => "Keeps playing similar songs when the queue ends",
        "Áudio mono" => "Mono audio",
        "Os dois lados tocam o mesmo som" => "Both sides play the same sound",
        "Balanço" => "Balance",
        "Buffer de rede" => "Network buffer",
        // Settings: equalizer
        "Ativar equalizador" => "Enable equalizer",
        "Ajusta graves, médios e agudos de tudo que toca" => "Adjusts bass, mids and treble of everything that plays",
        "Loudness adaptativo" => "Adaptive loudness",
        "Com volume baixo o ouvido perde graves e agudos; eles voltam aos poucos conforme você abaixa" => {
            "At low volume the ear loses bass and treble; they come back gradually as you turn it down"
        }
        "Plano" => "Flat",
        "Vocal" => "Vocal",
        "Rock suave" => "Soft rock",
        "Clube" => "Club",
        "Festa" => "Party",
        "Clássica" => "Classical",
        "Ao vivo" => "Live",
        "Grande salão" => "Large hall",
        "Suave" => "Soft",
        "Fones e notebook" => "Headphones & laptop",
        "Graves intensos" => "Full bass",
        "Agudos intensos" => "Full treble",
        "Graves e agudos" => "Bass & treble",
        // Settings: appearance
        "Idioma" => "Language",
        "Automático (sistema)" => "Automatic (system)",
        "Tema" => "Theme",
        "Seguir o sistema" => "Follow the system",
        "Escuro" => "Dark",
        "Claro" => "Light",
        "Abrir pasta de temas" => "Open themes folder",
        "Cores da capa" => "Album art colors",
        "Pinta as páginas com a cor da capa do álbum" => "Tints pages with the album art's color",
        "Efeito 3D nas capas" => "3D effect on covers",
        "Capas inclinam em direção ao cursor" => "Covers tilt towards the cursor",
        "Intensidade do 3D" => "3D intensity",
        "Reduzir animações" => "Reduce motion",
        "Desliga transições e animações de hover" => "Turns off transitions and hover animations",
        "Lista de faixas compacta" => "Compact track list",
        "Uma linha por faixa, sem capas" => "One line per track, no covers",
        "Barra lateral compacta" => "Compact sidebar",
        "Só os nomes, sem subtítulo" => "Names only, no subtitle",
        "Tamanho da interface" => "Interface size",
        // Settings: window
        "Fechar para a bandeja" => "Close to tray",
        "A música continua tocando com a janela fechada" => "Music keeps playing with the window closed",
        "Mini player sempre no topo" => "Mini player always on top",
        "Abrir no mini player" => "Open in the mini player",
        "Iniciar com o computador, na bandeja" => "Start with the computer, in the tray",
        "Toca e responde aos controles sem abrir janela: bem mais leve até você clicar no ícone" => {
            "Plays and answers media controls with no window: much lighter until you click the icon"
        }
        // Settings: Winamp
        "Skin do mini player" => "Mini player skin",
        "Nenhuma (player compacto do YtmFast)" => "None (YtmFast compact player)",
        "Velocidade" => "Speed",
        "Letra atrasada: adiantar 0,5 s" => "Lyrics running late: move them 0.5 s earlier",
        "Letra adiantada: atrasar 0,5 s" => "Lyrics running early: move them 0.5 s later",
        "Voltar ao tempo original" => "Back to the original timing",
        "Copiar link" => "Copy link",
        "Link copiado." => "Link copied.",
        "Avisar quando a música mudar" => "Notify when the song changes",
        "Notificação do sistema, só com a janela escondida ou em segundo plano" => "A system notification, only while the window is hidden or in the background",
        "Integrações" => "Integrations",
        "token de usuário" => "user token",
        "Registra o que você ouve no seu perfil. O token fica em" => "Logs what you listen to on your profile. Get the token at",
        "Seu resumo" => "Your summary",
        "Últimos 30 dias" => "Last 30 days",
        "Todo o histórico" => "All history",
        "Nada tocado nesse período." => "Nothing played in this period.",
        "{} músicas tocadas • {} ouvindo" => "{} songs played • {} of listening",
        "Artistas mais ouvidos" => "Top artists",
        "Músicas mais ouvidas" => "Top songs",
        "{} vez" => "{} play",
        "{} vezes" => "{} plays",
        "Skin aleatória" => "Random skin",
        "Sorteia outra skin cada vez que o mini player abre" => "Picks another skin each time the mini player opens",
        "Nenhuma skin instalada ainda. Baixe a clássica ou coloque arquivos .wsz na pasta de skins." => {
            "No skins installed yet. Download the classic one or put .wsz files in the skins folder."
        }
        "Clássica nas cores do tema" => "Classic in the theme's colors",
        "Estilo do player compacto" => "Compact player style",
        "Moderno" => "Modern",
        "Clássico" => "Classic",
        "Baixar skins:" => "Download skins:",
        "Abrir mini player" => "Open mini player",
        "Abrir pasta de skins" => "Open skins folder",
        "Mais skins no Winamp Skin Museum" => "More skins at the Winamp Skin Museum",
        "Escala da skin" => "Skin scale",
        // Settings: storage, network, about
        "Sessão e histórico" => "Session and history",
        "Guardar histórico" => "Keep history",
        "Fica só neste computador, nunca é enviado" => "Stays on this computer, never sent anywhere",
        "Limpar histórico" => "Clear history",
        "Cache" => "Cache",
        "Limpar cache" => "Clear cache",
        "Proxy" => "Proxy",
        "A reprodução usa o novo proxy na hora; a busca, depois de reiniciar o app." => {
            "Playback uses the new proxy right away; search, after restarting the app."
        }
        "No Linux, o playerctl controla o YtmFast pelo terminal (playerctl -p ytmfast play-pause)." => {
            "On Linux, playerctl controls YtmFast from the terminal (playerctl -p ytmfast play-pause)."
        }
        // Shortcuts list
        "Espaço" => "Space",
        "Tocar / pausar" => "Play / pause",
        "Faixa anterior / próxima" => "Previous / next track",
        "Voltar / avançar 10 s" => "Back / forward 10 s",
        "Voltar / avançar 5 s" => "Back / forward 5 s",
        "Curtir a faixa atual" => "Like the current track",
        "Aleatório / repetir" => "Shuffle / repeat",
        "Ctrl + F ou /" => "Ctrl + F or /",
        "Mostrar / esconder barra lateral" => "Show / hide sidebar",
        "Voltar / avançar entre telas" => "Back / forward between pages",
        "Início / músicas curtidas" => "Home / liked songs",
        "Artista / álbum da faixa atual" => "Current track's artist / album",
        "Tela cheia (no vídeo)" => "Full screen (in video)",
        "? ou Ctrl + /" => "? or Ctrl + /",
        "Esta lista" => "This list",
        // Tray
        "Mostrar YtmFast" => "Show YtmFast",
        " · pausado" => " · paused",
        // Terminal
        "O YtmFast não está aberto." => "YtmFast isn't running.",
        "Comando desconhecido: {}\n\n{}" => "Unknown command: {}\n\n{}",
        "comando desconhecido: {}" => "unknown command: {}",
        "informe o link" => "give the link",
        "informe os segundos" => "give the seconds",
        "informe o volume (0-100)" => "give the volume (0-100)",
        "link não reconhecido" => "link not recognized",
        "sem resposta" => "no answer",
        "pausado" => "paused",
        "tocando" => "playing",
        "Buscando…" => "Searching…",
        "Carregando {}…" => "Loading {}…",
        "Playlist \"{}\" criada" => "Playlist \"{}\" created",
        "Erro: {}" => "Error: {}",
        "Entrando…" => "Signing in…",
        "ou" => "or",
        "vazio = do sistema · http://host:porta" => "empty = system's · http://host:port",
        "{} inscritos" => "{} subscribers",
        "0,75×" => "0.75×",
        "1,25×" => "1.25×",
        "1,5×" => "1.5×",
        "1,75×" => "1.75×",
        "não encontrado" => "not found",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_and_fill() {
        apply("en");
        assert_eq!(tr("Fila"), "Queue");
        assert_eq!(tr("Rick Astley"), "Rick Astley");
        assert_eq!(crate::tf!("Ir para {}", "Rick Astley"), "Go to Rick Astley");
        assert_eq!(fill("{} e {}", &[&1, &"b"]), "1 e b");
        apply("pt");
        assert_eq!(tr("Fila"), "Fila");
        assert_eq!(crate::tf!("Ir para {}", "x"), "Ir para x");
    }

    /// Every `tr("…")` / `tf!("…")` text in the code has an English entry, so nothing new slips
    /// through in Portuguese.
    #[test]
    fn every_text_is_translated() {
        // Written the same way in both languages.
        const SAME: [&str; 1] = ["LCD"];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = vec![];
        // This file only holds the table (and these tests' own samples).
        for e in std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name() != "i18n.rs") {
            let src = std::fs::read_to_string(e.path()).unwrap();
            for open in ["tr(\"", "tf!(\""] {
                for (i, _) in src.match_indices(open) {
                    // The literal up to its closing quote, unescaping \" and \n.
                    let (mut text, mut chars) = (String::new(), src[i + open.len()..].chars());
                    while let Some(c) = chars.next() {
                        match c {
                            '"' => break,
                            '\\' => text.push(match chars.next() {
                                Some('n') => '\n',
                                Some(c) => c,
                                None => break,
                            }),
                            c => text.push(c),
                        }
                    }
                    if text.chars().any(char::is_alphabetic) && en(&text).is_none() && !SAME.contains(&text.as_str()) {
                        missing.push(format!("{}: {text:?}", e.file_name().to_string_lossy()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "no English text for:\n{}", missing.join("\n"));
    }
}
