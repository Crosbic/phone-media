use windows::ApplicationModel::AppInfo;
use windows::Foundation::TimeSpan;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::core::{HSTRING, Result};

const TARGET: &str = "phonemedia";

fn main() -> Result<()> {
    let manager = Manager::RequestAsync()?.join()?;
    let current = manager
        .GetCurrentSession()
        .and_then(|session| session.SourceAppUserModelId())
        .unwrap_or_default();
    let sessions: Vec<Session> = manager.GetSessions()?.into_iter().collect();

    println!("Сессий: {}", sessions.len());
    for session in &sessions {
        describe(session, &current)?;
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        return Ok(());
    };
    let Some(target) = sessions.iter().find(|session| {
        session.SourceAppUserModelId().is_ok_and(|app| {
            app.to_string()
                .to_lowercase()
                .replace('-', "")
                .contains(TARGET)
        })
    }) else {
        eprintln!("Сессия {TARGET} не найдена — программа запущена?");
        std::process::exit(1);
    };

    let operation = match command.as_str() {
        "play" => target.TryPlayAsync()?,
        "pause" => target.TryPauseAsync()?,
        "toggle" => target.TryTogglePlayPauseAsync()?,
        "stop" => target.TryStopAsync()?,
        "next" => target.TrySkipNextAsync()?,
        "prev" => target.TrySkipPreviousAsync()?,
        "seek" => {
            let seconds: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(60.0);
            target.TryChangePlaybackPositionAsync((seconds * 1e7) as i64)?
        }
        other => {
            eprintln!("Неизвестная команда: {other}");
            std::process::exit(2);
        }
    };
    let accepted = operation.join()?;
    println!(
        "{command}: {}",
        if accepted {
            "принято"
        } else {
            "отклонено"
        }
    );
    Ok(())
}

fn describe(session: &Session, current: &HSTRING) -> Result<()> {
    let app = session.SourceAppUserModelId()?;
    let properties = session.TryGetMediaPropertiesAsync()?.join()?;
    let status = session.GetPlaybackInfo()?.PlaybackStatus()?;
    let timeline = session.GetTimelineProperties()?;
    let shown_as = AppInfo::GetFromAppUserModelId(&app)
        .and_then(|info| info.DisplayInfo()?.DisplayName())
        .map_or_else(
            |error| format!("имя не найдено: {}", error.message()),
            |name| name.to_string(),
        );
    println!(
        "{} {app} («{shown_as}»): {} — {} [{}] {} / {}, обложка: {}",
        if &app == current { "*" } else { " " },
        properties.Artist()?,
        properties.Title()?,
        status_name(status),
        mmss(timeline.Position()?),
        mmss(timeline.EndTime()?),
        if properties.Thumbnail().is_ok() {
            "есть"
        } else {
            "нет"
        },
    );
    Ok(())
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Playing => "играет",
        Status::Paused => "пауза",
        Status::Stopped => "остановлено",
        Status::Changing => "меняется",
        Status::Opened => "открыто",
        Status::Closed => "закрыто",
        _ => "?",
    }
}

fn mmss(time: TimeSpan) -> String {
    let seconds = time.Duration.max(0) / 10_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
