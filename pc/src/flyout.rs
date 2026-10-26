use std::sync::mpsc::Sender;
use std::time::Duration;

use windows::Foundation::TypedEventHandler;
use windows::Media::Playback::MediaPlayer;
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::core::{HSTRING, Result};

use crate::Event;
use crate::phone::Player;

#[derive(PartialEq)]
struct Track {
    title: String,
    artist: String,
    album: String,
}

pub struct Flyout {
    _player: MediaPlayer,
    smtc: SystemMediaTransportControls,
    track: Option<Track>,
    art: Option<Vec<u8>>,
}

impl Flyout {
    pub fn new(events: Sender<Event>) -> Result<Self> {
        let player = MediaPlayer::new()?;
        player.CommandManager()?.SetIsEnabled(false)?;
        let smtc = player.SystemMediaTransportControls()?;
        smtc.SetIsEnabled(false)?;

        let buttons = events.clone();
        smtc.ButtonPressed(&TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, args| {
            let _ = buttons.send(Event::Button(args.ok()?.Button()?));
            Ok(())
        }))?;
        smtc.PlaybackPositionChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, args| {
            let ticks = args.ok()?.RequestedPlaybackPosition()?.Duration;
            let _ = events.send(Event::Seek(Duration::from_nanos(ticks.max(0) as u64 * 100)));
            Ok(())
        }))?;

        Ok(Self {
            _player: player,
            smtc,
            track: None,
            art: None,
        })
    }

    pub fn show(&mut self, player: &Player) -> Result<()> {
        let smtc = &self.smtc;
        smtc.SetIsEnabled(true)?;
        smtc.SetIsPlayEnabled(player.can("play") || player.can("playPause"))?;
        smtc.SetIsPauseEnabled(player.can("pause") || player.can("playPause"))?;
        smtc.SetIsStopEnabled(player.can("stop"))?;
        smtc.SetIsNextEnabled(player.can("next"))?;
        smtc.SetIsPreviousEnabled(player.can("prev"))?;

        let track = Track {
            title: player.title.clone(),
            artist: player.artist.clone(),
            album: player.album.clone(),
        };
        if self.track.as_ref() != Some(&track) {
            self.track = Some(track);
            self.update_display()?;
        }

        smtc.SetPlaybackStatus(if player.playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        })?;
        let length = Duration::from_millis(player.duration_ms);
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(Duration::ZERO.into())?;
        timeline.SetEndTime(length.into())?;
        timeline.SetMinSeekTime(Duration::ZERO.into())?;
        timeline.SetMaxSeekTime(
            if player.can("seek") {
                length
            } else {
                Duration::ZERO
            }
            .into(),
        )?;
        timeline.SetPosition(Duration::from_millis(player.position_ms).min(length).into())?;
        smtc.UpdateTimelineProperties(&timeline)
    }

    pub fn set_art(&mut self, jpeg: Option<Vec<u8>>) -> Result<()> {
        self.art = jpeg;
        self.update_display()
    }

    pub fn hide(&mut self) -> Result<()> {
        self.track = None;
        self.art = None;
        let updater = self.smtc.DisplayUpdater()?;
        updater.ClearAll()?;
        updater.Update()?;
        self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
        self.smtc.SetIsEnabled(false)
    }

    fn update_display(&self) -> Result<()> {
        let Some(track) = &self.track else {
            return Ok(());
        };
        let updater = self.smtc.DisplayUpdater()?;
        updater.ClearAll()?;
        updater.SetType(MediaPlaybackType::Music)?;
        let music = updater.MusicProperties()?;
        music.SetTitle(&HSTRING::from(track.title.as_str()))?;
        music.SetArtist(&HSTRING::from(track.artist.as_str()))?;
        music.SetAlbumTitle(&HSTRING::from(track.album.as_str()))?;
        if let Some(jpeg) = &self.art {
            updater.SetThumbnail(&image(jpeg)?)?;
        }
        updater.Update()
    }
}

fn image(bytes: &[u8]) -> Result<RandomAccessStreamReference> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.FlushAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    RandomAccessStreamReference::CreateFromStream(&stream)
}
