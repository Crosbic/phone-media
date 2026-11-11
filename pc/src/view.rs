use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Media::SystemMediaTransportControlsButton as Button;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::Foundation::{COLORREF, D2DERR_RECREATE_TARGET, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_ROUND, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_DEFAULT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dwm::{
    DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_BORDER_COLOR, DWMWA_FORCE_ICONIC_REPRESENTATION,
    DWMWA_HAS_ICONIC_BITMAP, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TRANSITIONS_FORCEDISABLED,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea, DwmInvalidateIconicBitmaps,
    DwmSetIconicLivePreviewBitmap, DwmSetIconicThumbnail, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, HBITMAP, HDC, InvalidateRect, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICBitmapSource,
    IWICImagingFactory, WICBitmapCacheOnLoad, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeMedianCut,
    WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Controls::{
    BP_PAINTPARAMS, BPBF_TOPDOWNDIB, BPPF_ERASE, BeginBufferedPaint, BufferedPaintInit,
    EndBufferedPaint, LoadIconWithScaleDown, MARGINS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GWL_EXSTYLE, GetClientRect, GetWindowLongPtrW, HICON,
    ICONINFO, LWA_ALPHA, SPI_GETCLIENTAREAANIMATION, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetLayeredWindowAttributes,
    SetWindowLongPtrW, SetWindowPos, SystemParametersInfoW, WS_EX_LAYERED,
};
use windows::core::{BOOL, HSTRING, PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::tray::{Status, VolumeTarget};
use crate::{Event, VolumeChange, log};

pub const WIDTH: f32 = 320.0;
const PAD: f32 = 16.0;
const ART: f32 = 64.0;
const ICON: f32 = 40.0;
const PLACEHOLDER: f32 = 0.625;
const BUTTON_STEP: f32 = 56.0;
const ART_MAX: f32 = 480.0;
const SIDE_SHARE: f32 = 0.5;
const ART_GAP: f32 = 12.0;
const TEXT_GAP: f32 = 8.0;
const FIT_TEXT: f32 = 58.0;
const FIT_BELOW: f32 = 80.0 + VOLUME_ROW;
const THUMB_HEIGHT: f32 = 100.0;
const THUMB_MIN_WIDTH: f32 = 200.0;
const THUMB_PAD: f32 = 10.0;
const THUMB_BAR_GAP: f32 = 8.0;
const ICON_CONNECTED: usize = 1;
const ICON_DISCONNECTED: usize = 2;
const VOLUME_ROW: f32 = 20.0;
const SLIDE_EVERY: Duration = Duration::from_millis(40);
const CONTROLS_GAP: f32 = 24.0;
const PROGRESS_GAP: f32 = 30.0;
const MAX_NOTCHES: u32 = 30;
const NOTCH_GAP: f32 = 2.0;
const VOLUME_HALF: f32 = BUTTON_STEP + 20.0;
pub const OSD_WIDTH: f32 = 300.0;
const OSD_HEIGHT: f32 = 48.0;

#[derive(Clone, Copy)]
pub struct Look {
    pub light: bool,
    pub acrylic: bool,
    pub accent: u32,
    pub tint: Option<u32>,
    pub animated: bool,
}

impl Look {
    fn current(settings: Option<&UISettings>, backdrop: bool) -> Self {
        let light = personalize("SystemUsesLightTheme") == Some(1);
        let transparency = personalize("EnableTransparency") != Some(0);
        let system = |kind: UIColorType| {
            settings
                .and_then(|settings| settings.GetColorValue(kind).ok())
                .map(|c| ((c.R as u32) << 16) | ((c.G as u32) << 8) | c.B as u32)
        };
        let accent = if light {
            system(UIColorType::AccentDark1).unwrap_or(0x137a7f)
        } else {
            system(UIColorType::AccentLight2).unwrap_or(0x86cecb)
        };
        let tint = if personalize("ColorPrevalence") == Some(1) {
            system(UIColorType::AccentDark2)
        } else {
            None
        };
        let mut animated = BOOL(1);
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some(&mut animated as *mut BOOL as _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        Look {
            light,
            acrylic: backdrop && transparency,
            accent,
            tint,
            animated: animated.as_bool(),
        }
    }
}

struct Palette {
    background: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    secondary: D2D1_COLOR_F,
    tertiary: D2D1_COLOR_F,
    disabled: D2D1_COLOR_F,
    rail: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    thumb: D2D1_COLOR_F,
    edge: D2D1_COLOR_F,
    subtle: D2D1_COLOR_F,
    pressed: D2D1_COLOR_F,
    border: u32,
}

impl Palette {
    fn new(look: &Look) -> Self {
        let (ink, surface, thumb, border) = if look.light {
            (0x000000, 0xf9f9f9, 0xffffff, 0xdcdcdc)
        } else {
            (0xffffff, 0x2c2c2c, 0x454545, 0x3d3d3d)
        };
        let ink = |light: f32, dark: f32| color(ink, if look.light { light } else { dark });
        let background = match look.tint {
            Some(tint) => color(tint, if look.acrylic { 0.75 } else { 1.0 }),
            None => color(surface, if look.acrylic { 0.0 } else { 1.0 }),
        };
        Palette {
            background,
            text: ink(0.896, 1.0),
            secondary: ink(0.606, 0.786),
            tertiary: ink(0.446, 0.544),
            disabled: ink(0.361, 0.363),
            rail: ink(0.446, 0.544),
            accent: color(look.accent, 1.0),
            thumb: color(thumb, 1.0),
            edge: ink(0.12, 0.09),
            subtle: ink(0.0373, 0.0605),
            pressed: ink(0.0241, 0.0419),
            border,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    Previous,
    Toggle,
    Next,
    Bar,
    Volume,
    Speaker,
}

#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Primary,
    Secondary,
    Tertiary,
}

#[derive(Clone, Copy)]
enum Picture {
    Art,
    Connected,
    Disconnected,
}

#[derive(Clone, Copy, Default)]
struct Ui {
    hover: Option<Part>,
    pressed: Option<Part>,
    dragging: bool,
    sliding: bool,
}

#[derive(Clone, Copy, Default)]
struct Drag {
    seek: Option<f32>,
    volume: Option<f32>,
}

#[derive(Clone, Copy, PartialEq)]
struct Level {
    level: u32,
    max: u32,
    muted: bool,
    fixed: bool,
}

impl Level {
    fn fraction(self) -> f32 {
        self.level as f32 / self.max as f32
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Layout {
    Popup,
    Window,
    Osd,
}

#[derive(Clone)]
enum Content {
    Offline(String),
    Idle(String),
    Playing(Playing),
}

#[derive(Clone)]
struct Playing {
    title: String,
    artist: String,
    source: String,
    playing: bool,
    position: Duration,
    duration: Duration,
    can_previous: bool,
    can_next: bool,
    can_toggle: bool,
    can_seek: bool,
    art: Option<Arc<[u8]>>,
    volume: Option<Level>,
}

impl Content {
    fn from(status: &Status) -> Self {
        if !status.connected {
            return Content::Offline(status.phone.clone());
        }
        let phone = status.name.clone().unwrap_or_default();
        let Some((player, received)) = &status.player else {
            return Content::Idle(if phone.is_empty() {
                "Телефон подключён".to_string()
            } else {
                phone
            });
        };
        let duration = Duration::from_millis(player.duration_ms);
        let mut position = Duration::from_millis(player.position_ms);
        if player.playing {
            position += received.elapsed();
        }
        if !duration.is_zero() {
            position = position.min(duration);
        }
        let source = [player.app.as_str(), phone.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let toggle = if player.playing { "pause" } else { "play" };
        Content::Playing(Playing {
            title: if player.title.is_empty() {
                "Без названия".to_string()
            } else {
                player.title.clone()
            },
            artist: player.artist.clone(),
            source,
            playing: player.playing,
            position,
            duration,
            can_previous: player.can("prev"),
            can_next: player.can("next"),
            can_toggle: player.can(toggle) || player.can("playPause"),
            can_seek: player.can("seek") && !duration.is_zero(),
            art: status.art.clone(),
            volume: player
                .volume
                .filter(|_| player.volume_max > 0)
                .map(|level| Level {
                    level: level.min(player.volume_max),
                    max: player.volume_max,
                    muted: player.muted,
                    fixed: player.volume_fixed,
                }),
        })
    }

    fn art(&self) -> Option<&Arc<[u8]>> {
        match self {
            Content::Playing(playing) => playing.art.as_ref(),
            _ => None,
        }
    }
}

struct Text {
    layout: IDWriteTextLayout,
    at: Vector2,
    tone: Tone,
}

struct Control {
    part: Part,
    center: Vector2,
    enabled: bool,
}

struct Progress {
    bar: D2D_RECT_F,
    fraction: f32,
    seekable: bool,
}

struct VolumeRow {
    bar: D2D_RECT_F,
    fraction: f32,
    notches: u32,
    muted: bool,
    enabled: bool,
    speaker: Vector2,
}

#[derive(Clone, Copy)]
enum Frame {
    Natural,
    Osd,
    Preview { width: f32, height: f32 },
    Fixed { width: f32, height: f32 },
}

impl Frame {
    fn width(self) -> f32 {
        match self {
            Frame::Natural => WIDTH,
            Frame::Osd => OSD_WIDTH,
            Frame::Preview { width, .. } | Frame::Fixed { width, .. } => width,
        }
    }
}

struct Scene {
    width: f32,
    height: f32,
    picture: (Picture, D2D_RECT_F),
    texts: Vec<Text>,
    progress: Option<Progress>,
    controls: Vec<Control>,
    volume: Option<VolumeRow>,
    playing: bool,
}

type Lines = Vec<(IDWriteTextLayout, f32, Tone)>;

struct Resources {
    factory: ID2D1Factory,
    write: IDWriteFactory,
    wic: IWICImagingFactory,
    title: IDWriteTextFormat,
    body: IDWriteTextFormat,
    wrapped: IDWriteTextFormat,
    small: IDWriteTextFormat,
    play: ID2D1PathGeometry,
    pause: ID2D1PathGeometry,
    previous: ID2D1PathGeometry,
    next: ID2D1PathGeometry,
    round: ID2D1StrokeStyle,
    speaker: ID2D1PathGeometry,
    wave_low: ID2D1PathGeometry,
    wave_high: ID2D1PathGeometry,
    cross: ID2D1PathGeometry,
}

impl Resources {
    fn new() -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let family = font_family(&write);
            let format = |size: f32, weight: DWRITE_FONT_WEIGHT, wrap: bool| {
                let format = write.CreateTextFormat(
                    &family,
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("ru-ru"),
                )?;
                format.SetWordWrapping(if wrap {
                    DWRITE_WORD_WRAPPING_WRAP
                } else {
                    DWRITE_WORD_WRAPPING_NO_WRAP
                })?;
                let ellipsis = write.CreateEllipsisTrimmingSign(&format)?;
                format.SetTrimming(
                    &DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    },
                    &ellipsis,
                )?;
                Result::Ok(format)
            };
            let shape = |figures: &[&[(f32, f32)]]| {
                let path = factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                for figure in figures {
                    let points: Vec<Vector2> = figure.iter().map(|&(x, y)| v(x, y)).collect();
                    sink.BeginFigure(points[0], D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLines(&points[1..]);
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                }
                sink.Close()?;
                Result::Ok(path)
            };
            let stroke = |figures: &[Vec<(f32, f32)>]| {
                let path = factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                for figure in figures {
                    let points: Vec<Vector2> = figure.iter().map(|&(x, y)| v(x, y)).collect();
                    sink.BeginFigure(points[0], D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddLines(&points[1..]);
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
                sink.Close()?;
                Result::Ok(path)
            };
            let arc = |radius: f32| -> Vec<(f32, f32)> {
                (0..=8)
                    .map(|step| {
                        let angle = (-45.0 + 11.25 * step as f32).to_radians();
                        (radius * angle.cos(), radius * angle.sin())
                    })
                    .collect()
            };
            let round = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.0,
                    ..Default::default()
                },
                None,
            )?;
            Ok(Self {
                title: format(14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, false)?,
                body: format(12.0, DWRITE_FONT_WEIGHT_NORMAL, false)?,
                wrapped: format(12.0, DWRITE_FONT_WEIGHT_NORMAL, true)?,
                small: format(11.0, DWRITE_FONT_WEIGHT_NORMAL, false)?,
                play: shape(&[&[(-5.0, -7.0), (-5.0, 7.0), (7.0, 0.0)]])?,
                pause: shape(&[
                    &[(-6.0, -7.0), (-2.0, -7.0), (-2.0, 7.0), (-6.0, 7.0)],
                    &[(2.0, -7.0), (6.0, -7.0), (6.0, 7.0), (2.0, 7.0)],
                ])?,
                previous: shape(&[
                    &[(-7.0, -6.0), (-5.0, -6.0), (-5.0, 6.0), (-7.0, 6.0)],
                    &[(6.0, -6.0), (6.0, 6.0), (-3.0, 0.0)],
                ])?,
                next: shape(&[
                    &[(5.0, -6.0), (7.0, -6.0), (7.0, 6.0), (5.0, 6.0)],
                    &[(-6.0, -6.0), (-6.0, 6.0), (3.0, 0.0)],
                ])?,
                speaker: shape(&[&[
                    (-7.0, -2.5),
                    (-4.0, -2.5),
                    (0.0, -6.0),
                    (0.0, 6.0),
                    (-4.0, 2.5),
                    (-7.0, 2.5),
                ]])?,
                wave_low: stroke(&[arc(3.5)])?,
                wave_high: stroke(&[arc(7.0)])?,
                cross: stroke(&[vec![(3.0, -2.5), (8.0, 2.5)], vec![(3.0, 2.5), (8.0, -2.5)]])?,
                round,
                factory,
                write,
                wic,
            })
        }
    }

    fn text(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        width: f32,
        max_height: f32,
    ) -> Result<(IDWriteTextLayout, f32, f32)> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            self.write
                .CreateTextLayout(&wide, format, width, max_height)?
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        Ok((
            layout,
            metrics.widthIncludingTrailingWhitespace,
            metrics.height.min(max_height),
        ))
    }

    fn scene(&self, content: &Content, frame: Frame, drag: Drag) -> Result<Scene> {
        match content {
            Content::Offline(line) => {
                self.compact(Picture::Disconnected, "Телефон не подключён", line, frame)
            }
            Content::Idle(phone) => {
                self.compact(Picture::Connected, phone, "Ничего не играет", frame)
            }
            Content::Playing(playing) if let Frame::Preview { width, height } = frame => {
                self.thumbnail(playing, width, height)
            }
            Content::Playing(playing) if matches!(frame, Frame::Osd) => self.osd(playing, drag),
            Content::Playing(playing) => self.player(playing, frame, drag),
        }
    }

    fn volume_row(
        &self,
        level: Level,
        drag: Option<f32>,
        (left, right, middle): (f32, f32, f32),
        texts: &mut Vec<Text>,
    ) -> Result<VolumeRow> {
        let fraction = drag.unwrap_or(level.fraction()).clamp(0.0, 1.0);
        let steps = (fraction * level.max as f32).round();
        let percent = (steps * 100.0 / level.max as f32).round() as u32;
        let notched = level.max <= MAX_NOTCHES;
        let fraction = if notched {
            steps / level.max as f32
        } else {
            fraction
        };
        let (layout, _, height) = self.text(&percent.to_string(), &self.small, 40.0, 20.0)?;
        texts.push(Text {
            layout,
            at: v(right + 10.0, middle - height / 2.0),
            tone: Tone::Tertiary,
        });
        Ok(VolumeRow {
            bar: D2D_RECT_F {
                left,
                top: middle - 2.0,
                right,
                bottom: middle + 2.0,
            },
            fraction,
            notches: if notched { level.max } else { 0 },
            muted: level.muted && drag.is_none(),
            enabled: !level.fixed,
            speaker: v(left - 22.0, middle),
        })
    }

    fn osd(&self, playing: &Playing, drag: Drag) -> Result<Scene> {
        let mut texts = Vec::new();
        let volume = match playing.volume {
            Some(level) => Some(self.volume_row(
                level,
                drag.volume,
                (80.0, OSD_WIDTH - 50.0, OSD_HEIGHT / 2.0),
                &mut texts,
            )?),
            None => None,
        };
        Ok(Scene {
            width: OSD_WIDTH,
            height: OSD_HEIGHT,
            picture: (Picture::Connected, rect(14.0, 12.0, 24.0, 24.0)),
            texts,
            progress: None,
            controls: Vec::new(),
            volume,
            playing: playing.playing,
        })
    }

    fn thumbnail(&self, playing: &Playing, width: f32, height: f32) -> Result<Scene> {
        let cover = (height - 2.0 * THUMB_PAD).min(width * 0.45).max(0.0);
        let x = THUMB_PAD + cover + ART_GAP;
        let room = (width - x - THUMB_PAD).max(1.0);
        let mut lines = vec![(playing.title.as_str(), &self.title, Tone::Primary)];
        if !playing.artist.is_empty() {
            lines.push((playing.artist.as_str(), &self.body, Tone::Secondary));
        }
        let (mut measured, mut block) = self.measure(&lines, room, false)?;
        let timed = !playing.duration.is_zero();
        if timed {
            let long = playing.duration >= Duration::from_secs(3600);
            let time = format!(
                "{} / {}",
                clock(playing.position, long),
                clock(playing.duration, long)
            );
            let (layout, _, line) = self.text(&time, &self.small, room, 20.0)?;
            measured.push((layout, line, Tone::Tertiary));
            block += 6.0 + line + THUMB_BAR_GAP + 4.0;
        }
        let count = measured.len();
        let mut y = ((height - block) / 2.0).max(0.0);
        let mut texts = Vec::new();
        for (index, (layout, line, tone)) in measured.into_iter().enumerate() {
            if timed && index == count - 1 {
                y += 4.0;
            }
            texts.push(Text {
                layout,
                at: v(x, y),
                tone,
            });
            y += line + 2.0;
        }
        let progress = timed.then(|| Progress {
            bar: rect(x, y - 2.0 + THUMB_BAR_GAP, room, 4.0),
            fraction: (playing.position.as_secs_f32() / playing.duration.as_secs_f32())
                .clamp(0.0, 1.0),
            seekable: false,
        });
        Ok(Scene {
            width,
            height,
            picture: (
                Picture::Art,
                rect(THUMB_PAD, (height - cover) / 2.0, cover, cover),
            ),
            texts,
            progress,
            controls: Vec::new(),
            volume: None,
            playing: playing.playing,
        })
    }

    fn compact(&self, picture: Picture, title: &str, line: &str, frame: Frame) -> Result<Scene> {
        let width = frame.width();
        let x = PAD + ICON + 12.0;
        let room = (width - x - PAD).max(1.0);
        let (title, _, title_height) = self.text(title, &self.title, room, 40.0)?;
        let (line, _, line_height) = self.text(line, &self.wrapped, room, 34.0)?;
        let block = title_height + 2.0 + line_height;
        let content = block.max(ICON);
        let height = match frame {
            Frame::Natural | Frame::Osd => PAD * 2.0 + content,
            Frame::Preview { height, .. } | Frame::Fixed { height, .. } => height,
        };
        let top = (height - content) / 2.0;
        let text_top = top + (content - block) / 2.0;
        Ok(Scene {
            width,
            height,
            picture: (picture, rect(PAD, top + (content - ICON) / 2.0, ICON, ICON)),
            texts: vec![
                Text {
                    layout: title,
                    at: v(x, text_top),
                    tone: Tone::Primary,
                },
                Text {
                    layout: line,
                    at: v(x, text_top + title_height + 2.0),
                    tone: Tone::Secondary,
                },
            ],
            progress: None,
            controls: Vec::new(),
            volume: None,
            playing: false,
        })
    }

    fn lines<'a>(&'a self, playing: &'a Playing) -> Vec<(&'a str, &'a IDWriteTextFormat, Tone)> {
        let mut lines = vec![(playing.title.as_str(), &self.title, Tone::Primary)];
        if !playing.artist.is_empty() {
            lines.push((playing.artist.as_str(), &self.body, Tone::Secondary));
        }
        if !playing.source.is_empty() {
            lines.push((playing.source.as_str(), &self.body, Tone::Tertiary));
        }
        lines
    }

    fn measure(
        &self,
        lines: &[(&str, &IDWriteTextFormat, Tone)],
        room: f32,
        centered: bool,
    ) -> Result<(Lines, f32)> {
        let mut measured = Vec::new();
        for &(text, format, tone) in lines {
            let (layout, _, height) = self.text(text, format, room.max(1.0), 40.0)?;
            if centered {
                unsafe { layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)? };
            }
            measured.push((layout, height, tone));
        }
        let block = measured.iter().map(|line| line.1).sum::<f32>()
            + 2.0 * measured.len().saturating_sub(1) as f32;
        Ok((measured, block))
    }

    fn player(&self, playing: &Playing, frame: Frame, drag: Drag) -> Result<Scene> {
        let width = frame.width();
        let below = if playing.duration.is_zero() {
            54.0
        } else {
            80.0
        } + if playing.volume.is_some() {
            VOLUME_ROW
        } else {
            0.0
        };
        let lines = self.lines(playing);
        let mut texts = Vec::new();
        let mut stack = |measured: Lines, x: f32, mut y: f32| {
            for (layout, height, tone) in measured {
                texts.push(Text {
                    layout,
                    at: v(x, y),
                    tone,
                });
                y += height + 2.0;
            }
        };
        let (art, bottom, height) = match frame {
            Frame::Natural | Frame::Osd | Frame::Preview { .. } => {
                let x = PAD + ART + 12.0;
                let (measured, block) = self.measure(&lines, width - x - PAD, false)?;
                stack(measured, x, PAD + ((ART - block) / 2.0).max(0.0));
                let bottom = PAD + ART.max(block);
                (rect(PAD, PAD, ART, ART), bottom, bottom + below)
            }
            Frame::Fixed { height, .. } => {
                let room = width - 2.0 * PAD;
                let header = (height - PAD - below).max(0.0);
                let side = header.min(room * SIDE_SHARE).min(ART_MAX);
                let (centered, block) = self.measure(&lines, room, true)?;
                let top_art = room
                    .min(ART_MAX)
                    .min(height - PAD - ART_GAP - block - TEXT_GAP - below);
                if room <= ART_MAX && top_art > side {
                    let slack = height - below - TEXT_GAP - PAD - top_art - ART_GAP - block;
                    let top = PAD + (slack / 2.0).max(0.0);
                    stack(centered, PAD, top + top_art + ART_GAP);
                    let art = rect((width - top_art) / 2.0, top, top_art, top_art);
                    (art, height - below, height)
                } else {
                    let x = PAD + side + ART_GAP;
                    let (measured, block) = self.measure(&lines, width - x - PAD, false)?;
                    stack(measured, x, PAD + ((header - block) / 2.0).max(0.0));
                    let art = rect(PAD, PAD + (header - side) / 2.0, side, side);
                    (art, PAD + header, height)
                }
            }
        };

        let buttons = bottom + CONTROLS_GAP;
        let mut progress = None;
        if !playing.duration.is_zero() {
            let natural = playing.position.as_secs_f32() / playing.duration.as_secs_f32();
            let fraction = drag.seek.unwrap_or(natural).clamp(0.0, 1.0);
            let shown = match drag.seek {
                Some(_) => playing.duration.mul_f32(fraction),
                None => playing.position,
            };
            let long = playing.duration >= Duration::from_secs(3600);
            let (left, left_width, label_height) =
                self.text(&clock(shown, long), &self.small, 100.0, 20.0)?;
            let (right, right_width, _) =
                self.text(&clock(playing.duration, long), &self.small, 100.0, 20.0)?;
            let slot = left_width.max(right_width);
            let middle = buttons + PROGRESS_GAP;
            texts.push(Text {
                layout: left,
                at: v(PAD, middle - label_height / 2.0),
                tone: Tone::Tertiary,
            });
            texts.push(Text {
                layout: right,
                at: v(width - PAD - right_width, middle - label_height / 2.0),
                tone: Tone::Tertiary,
            });
            progress = Some(Progress {
                bar: D2D_RECT_F {
                    left: PAD + slot + 10.0,
                    top: middle - 2.0,
                    right: width - PAD - slot - 10.0,
                    bottom: middle + 2.0,
                },
                fraction,
                seekable: playing.can_seek,
            });
        }

        let middle = buttons;
        let controls = vec![
            Control {
                part: Part::Previous,
                center: v(width / 2.0 - BUTTON_STEP, middle),
                enabled: playing.can_previous,
            },
            Control {
                part: Part::Toggle,
                center: v(width / 2.0, middle),
                enabled: playing.can_toggle,
            },
            Control {
                part: Part::Next,
                center: v(width / 2.0 + BUTTON_STEP, middle),
                enabled: playing.can_next,
            },
        ];
        let volume = match playing.volume {
            Some(level) => Some(self.volume_row(
                level,
                drag.volume,
                (
                    width / 2.0 - VOLUME_HALF,
                    width / 2.0 + VOLUME_HALF,
                    height - VOLUME_ROW,
                ),
                &mut texts,
            )?),
            None => None,
        };
        Ok(Scene {
            width,
            height,
            picture: (Picture::Art, art),
            texts,
            progress,
            controls,
            volume,
            playing: playing.playing,
        })
    }

    fn draw(
        &self,
        target: &ID2D1RenderTarget,
        scene: &Scene,
        palette: &Palette,
        images: &Images,
        ui: Ui,
        opacity: f32,
    ) -> Result<()> {
        unsafe {
            let brush = target.CreateSolidColorBrush(&palette.background, None)?;
            brush.SetOpacity(opacity);
            target.SetTransform(&Matrix3x2::identity());
            target.Clear(Some(&color(0, 0.0)));
            target.FillRectangle(&rect(0.0, 0.0, scene.width, scene.height), &brush);
            self.picture(target, &brush, palette, images, scene.picture, opacity)?;

            for text in &scene.texts {
                let tone = match text.tone {
                    Tone::Primary => palette.text,
                    Tone::Secondary => palette.secondary,
                    Tone::Tertiary => palette.tertiary,
                };
                brush.SetColor(&tone);
                target.DrawTextLayout(
                    text.at,
                    &text.layout,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                );
            }

            if let Some(progress) = &scene.progress {
                let bar = progress.bar;
                brush.SetColor(&palette.rail);
                target.FillRoundedRectangle(&rounded(bar, 2.0), &brush);
                let x = bar.left + (bar.right - bar.left) * progress.fraction;
                brush.SetColor(&palette.accent);
                target.FillRoundedRectangle(&rounded(D2D_RECT_F { right: x, ..bar }, 2.0), &brush);
                if progress.seekable {
                    let middle = (bar.top + bar.bottom) / 2.0;
                    let grown = ui.dragging || ui.hover == Some(Part::Bar);
                    brush.SetColor(&palette.thumb);
                    target.FillEllipse(&circle(x, middle, 8.0), &brush);
                    brush.SetColor(&palette.edge);
                    target.DrawEllipse(&circle(x, middle, 8.0), &brush, 1.0, None);
                    brush.SetColor(&palette.accent);
                    target.FillEllipse(&circle(x, middle, if grown { 5.5 } else { 4.5 }), &brush);
                }
            }

            for control in &scene.controls {
                let c = control.center;
                if control.enabled && ui.hover == Some(control.part) {
                    brush.SetColor(if ui.pressed == Some(control.part) {
                        &palette.pressed
                    } else {
                        &palette.subtle
                    });
                    target.FillRoundedRectangle(
                        &rounded(rect(c.X - 20.0, c.Y - 16.0, 40.0, 32.0), 4.0),
                        &brush,
                    );
                }
                brush.SetColor(if control.enabled {
                    &palette.text
                } else {
                    &palette.disabled
                });
                let shape = match control.part {
                    Part::Previous => &self.previous,
                    Part::Next => &self.next,
                    Part::Toggle if scene.playing => &self.pause,
                    Part::Toggle => &self.play,
                    Part::Bar | Part::Volume | Part::Speaker => continue,
                };
                target.SetTransform(&Matrix3x2::translation(c.X, c.Y));
                target.FillGeometry(shape, &brush, None);
                target.DrawGeometry(shape, &brush, 2.0, &self.round);
                target.SetTransform(&Matrix3x2::identity());
            }

            if let Some(volume) = &scene.volume {
                let c = volume.speaker;
                if volume.enabled && ui.hover == Some(Part::Speaker) {
                    brush.SetColor(if ui.pressed == Some(Part::Speaker) {
                        &palette.pressed
                    } else {
                        &palette.subtle
                    });
                    target.FillRoundedRectangle(
                        &rounded(rect(c.X - 14.0, c.Y - 12.0, 28.0, 24.0), 4.0),
                        &brush,
                    );
                }
                brush.SetColor(if volume.enabled {
                    &palette.text
                } else {
                    &palette.disabled
                });
                target.SetTransform(&Matrix3x2::translation(c.X, c.Y));
                target.FillGeometry(&self.speaker, &brush, None);
                target.DrawGeometry(&self.speaker, &brush, 1.0, &self.round);
                if volume.muted || volume.fraction <= 0.0 {
                    target.DrawGeometry(&self.cross, &brush, 1.5, &self.round);
                } else {
                    target.DrawGeometry(&self.wave_low, &brush, 1.5, &self.round);
                    if volume.fraction > 0.5 {
                        target.DrawGeometry(&self.wave_high, &brush, 1.5, &self.round);
                    }
                }
                target.SetTransform(&Matrix3x2::identity());

                let bar = volume.bar;
                let x = bar.left + (bar.right - bar.left) * volume.fraction;
                let filled = if volume.muted || !volume.enabled {
                    &palette.disabled
                } else {
                    &palette.accent
                };
                if volume.notches > 1 {
                    let width = (bar.right - bar.left) / volume.notches as f32;
                    for step in 0..volume.notches {
                        let left = bar.left + width * step as f32;
                        let segment = D2D_RECT_F {
                            left: left + if step == 0 { 0.0 } else { NOTCH_GAP / 2.0 },
                            right: left + width
                                - if step + 1 == volume.notches {
                                    0.0
                                } else {
                                    NOTCH_GAP / 2.0
                                },
                            ..bar
                        };
                        let on = left + width / 2.0 < x;
                        brush.SetColor(if on { filled } else { &palette.rail });
                        target.FillRoundedRectangle(&rounded(segment, 1.5), &brush);
                    }
                } else {
                    brush.SetColor(&palette.rail);
                    target.FillRoundedRectangle(&rounded(bar, 2.0), &brush);
                    brush.SetColor(filled);
                    target.FillRoundedRectangle(
                        &rounded(D2D_RECT_F { right: x, ..bar }, 2.0),
                        &brush,
                    );
                }
                if volume.enabled {
                    let middle = (bar.top + bar.bottom) / 2.0;
                    let grown = ui.sliding || ui.hover == Some(Part::Volume);
                    brush.SetColor(&palette.thumb);
                    target.FillEllipse(&circle(x, middle, 8.0), &brush);
                    brush.SetColor(&palette.edge);
                    target.DrawEllipse(&circle(x, middle, 8.0), &brush, 1.0, None);
                    brush.SetColor(if volume.muted {
                        &palette.disabled
                    } else {
                        &palette.accent
                    });
                    target.FillEllipse(&circle(x, middle, if grown { 5.5 } else { 4.5 }), &brush);
                }
            }
        }
        Ok(())
    }

    fn picture(
        &self,
        target: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        palette: &Palette,
        images: &Images,
        (picture, frame): (Picture, D2D_RECT_F),
        opacity: f32,
    ) -> Result<()> {
        let icon = |bitmap: Option<&ID2D1Bitmap>, frame: D2D_RECT_F| unsafe {
            if let Some(bitmap) = bitmap {
                target.DrawBitmap(
                    bitmap,
                    Some(&frame),
                    opacity,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
            }
        };
        let side = frame.right - frame.left;
        let radius = if side > 100.0 { 8.0 } else { 6.0 };
        match picture {
            Picture::Connected => icon(images.icon(ICON_CONNECTED), frame),
            Picture::Disconnected => icon(images.icon(ICON_DISCONNECTED), frame),
            Picture::Art => unsafe {
                if let Some(art) = images.art() {
                    let size = art.GetSize();
                    let fill = target.CreateBitmapBrush(
                        art,
                        Some(&D2D1_BITMAP_BRUSH_PROPERTIES {
                            extendModeX: D2D1_EXTEND_MODE_CLAMP,
                            extendModeY: D2D1_EXTEND_MODE_CLAMP,
                            interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        }),
                        None,
                    )?;
                    fill.SetOpacity(opacity);
                    fill.SetTransform(
                        &(Matrix3x2::scale(side / size.width, side / size.height)
                            * Matrix3x2::translation(frame.left, frame.top)),
                    );
                    target.FillRoundedRectangle(&rounded(frame, radius), &fill);
                } else {
                    brush.SetColor(&palette.subtle);
                    target.FillRoundedRectangle(&rounded(frame, radius), brush);
                    let mark = side * PLACEHOLDER;
                    let inset = (side - mark) / 2.0;
                    icon(
                        images.icon(ICON_CONNECTED),
                        rect(frame.left + inset, frame.top + inset, mark, mark),
                    );
                }
            },
        }
        Ok(())
    }

    fn bitmap(&self, target: &ID2D1RenderTarget, source: &IWICBitmapSource) -> Result<ID2D1Bitmap> {
        unsafe {
            let converter = self.wic.CreateFormatConverter()?;
            converter.Initialize(
                source,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )?;
            target.CreateBitmapFromWicBitmap(&converter, None)
        }
    }

    fn icon(&self, target: &ID2D1RenderTarget, id: usize, size: u32) -> Result<ID2D1Bitmap> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let icon = LoadIconWithScaleDown(
                Some(instance.into()),
                PCWSTR(id as _),
                size as i32,
                size as i32,
            )?;
            let source = self.wic.CreateBitmapFromHICON(icon);
            let _ = DestroyIcon(icon);
            self.bitmap(target, &source?.into())
        }
    }

    fn decode(&self, jpeg: &[u8]) -> Result<IWICBitmap> {
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromMemory(jpeg)?;
            let decoder = self.wic.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let (mut width, mut height) = (0, 0);
            frame.GetSize(&mut width, &mut height)?;
            let side = width.min(height);
            let clipper = self.wic.CreateBitmapClipper()?;
            clipper.Initialize(
                &frame,
                &WICRect {
                    X: ((width - side) / 2) as i32,
                    Y: ((height - side) / 2) as i32,
                    Width: side as i32,
                    Height: side as i32,
                },
            )?;
            self.wic
                .CreateBitmapFromSource(&clipper, WICBitmapCacheOnLoad)
        }
    }

    fn scaled(
        &self,
        target: &ID2D1RenderTarget,
        source: &IWICBitmap,
        size: u32,
    ) -> Result<ID2D1Bitmap> {
        unsafe {
            let scaler = self.wic.CreateBitmapScaler()?;
            scaler.Initialize(
                source,
                size,
                size,
                WICBitmapInterpolationModeHighQualityCubic,
            )?;
            self.bitmap(target, &scaler.into())
        }
    }
}

#[derive(Default)]
struct Images {
    scale: f32,
    connected: Option<(u32, ID2D1Bitmap)>,
    disconnected: Option<(u32, ID2D1Bitmap)>,
    source: Option<(Arc<[u8]>, Option<IWICBitmap>)>,
    art: Option<(u32, ID2D1Bitmap)>,
}

impl Images {
    fn prepare(
        &mut self,
        resources: &Resources,
        target: &ID2D1RenderTarget,
        scale: f32,
        scene: &Scene,
        art: Option<&Arc<[u8]>>,
    ) {
        if self.scale != scale {
            *self = Images {
                scale,
                ..Images::default()
            };
        }
        let pixels = |length: f32| (length * scale).round().max(1.0) as u32;
        let (picture, frame) = scene.picture;
        let side = frame.right - frame.left;
        let (slot, id, size) = match (picture, art) {
            (Picture::Connected, _) => (&mut self.connected, ICON_CONNECTED, pixels(side)),
            (Picture::Disconnected, _) => (&mut self.disconnected, ICON_DISCONNECTED, pixels(side)),
            (Picture::Art, None) => {
                self.source = None;
                self.art = None;
                (
                    &mut self.connected,
                    ICON_CONNECTED,
                    pixels(side * PLACEHOLDER),
                )
            }
            (Picture::Art, Some(jpeg)) => {
                self.cover(resources, target, jpeg, pixels(side));
                return;
            }
        };
        if !slot.as_ref().is_some_and(|(cached, _)| *cached == size) {
            *slot = resources
                .icon(target, id, size)
                .ok()
                .map(|bitmap| (size, bitmap));
        }
    }

    fn cover(
        &mut self,
        resources: &Resources,
        target: &ID2D1RenderTarget,
        jpeg: &Arc<[u8]>,
        size: u32,
    ) {
        let known = self
            .source
            .as_ref()
            .is_some_and(|(cached, _)| Arc::ptr_eq(cached, jpeg));
        if !known {
            let decoded = resources.decode(jpeg);
            if let Err(error) = &decoded {
                log(format!("мини-плеер: не удалось показать обложку: {error}"));
            }
            self.source = Some((jpeg.clone(), decoded.ok()));
            self.art = None;
        }
        if self.art.as_ref().is_some_and(|(cached, _)| *cached == size) {
            return;
        }
        self.art = self
            .source
            .as_ref()
            .and_then(|(_, source)| source.as_ref())
            .and_then(|source| resources.scaled(target, source, size).ok())
            .map(|bitmap| (size, bitmap));
    }

    fn icon(&self, id: usize) -> Option<&ID2D1Bitmap> {
        let slot = if id == ICON_CONNECTED {
            &self.connected
        } else {
            &self.disconnected
        };
        slot.as_ref().map(|(_, bitmap)| bitmap)
    }

    fn art(&self) -> Option<&ID2D1Bitmap> {
        self.art.as_ref().map(|(_, bitmap)| bitmap)
    }
}

pub const COMMAND_PREVIOUS: u32 = 1;
pub const COMMAND_TOGGLE: u32 = 2;
pub const COMMAND_NEXT: u32 = 3;

#[derive(Clone, Copy, PartialEq)]
pub enum Glyph {
    Previous,
    Play,
    Pause,
    Next,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub struct Transport {
    pub playing: bool,
    pub previous: bool,
    pub toggle: bool,
    pub next: bool,
}

pub const VOLUME_HOLD: Duration = Duration::from_millis(1500);
pub const VOLUME_RECHECK: Duration = Duration::from_millis(400);

pub fn volume_step(max: u32) -> u32 {
    ((max as f32) * 0.02).round().max(1.0) as u32
}

pub fn change_volume(status: &Mutex<Status>, events: &Sender<Event>, change: VolumeChange) {
    let Ok(mut guard) = status.lock() else {
        return;
    };
    let status = &mut *guard;
    let target = status.volume_target;
    let Some((player, _)) = status.player.as_mut() else {
        return;
    };
    let (Some(current), max) = (player.volume, player.volume_max) else {
        return;
    };
    let held = target
        .filter(|target| target.at.elapsed() < VOLUME_HOLD && target.max == max)
        .map(|target| target.level);
    if max == 0 {
        return;
    }
    let base = held.unwrap_or(current);
    let level = match change {
        VolumeChange::Set(value) => value.min(max),
        VolumeChange::Up => (base + volume_step(max)).min(max),
        VolumeChange::Down => base.saturating_sub(volume_step(max)),
        VolumeChange::Mute => {
            player.muted = !player.muted;
            drop(guard);
            let _ = events.send(Event::Volume(VolumeChange::Mute));
            return;
        }
    };
    if level > 0 {
        player.muted = false;
    }
    player.volume = Some(level);
    status.volume_target = Some(VolumeTarget {
        level,
        max,
        at: Instant::now(),
        repeated: false,
    });
    drop(guard);
    let _ = events.send(Event::Volume(VolumeChange::Set(level)));
}

pub fn volume_controllable(status: &Status) -> bool {
    status.connected
        && status.player.as_ref().is_some_and(|(player, _)| {
            player.volume.is_some() && player.volume_max > 0 && !player.volume_fixed
        })
}

pub fn fit_height(width: f32) -> f32 {
    let room = width - 2.0 * PAD;
    let side = PAD + (room * SIDE_SHARE).min(ART_MAX) + FIT_BELOW;
    if room > ART_MAX {
        return side;
    }
    side.max(PAD + room + ART_GAP + FIT_TEXT + TEXT_GAP + FIT_BELOW)
}

pub fn tallest() -> f32 {
    fit_height(ART_MAX + 2.0 * PAD)
}

pub struct View {
    pub window: HWND,
    status: Arc<Mutex<Status>>,
    events: Sender<Event>,
    resources: Resources,
    settings: Option<UISettings>,
    backdrop: bool,
    acrylic: Option<bool>,
    fade_window: bool,
    layout: Layout,
    pub look: Look,
    target: Option<ID2D1DCRenderTarget>,
    images: Images,
    scene: Option<Scene>,
    pub scale: f32,
    ui: Ui,
    drag: Option<f32>,
    volume_drag: Option<f32>,
    volume_sent: Option<u32>,
    volume_sent_at: Option<Instant>,
    wheel_rest: i32,
    pub opacity: f32,
    tracking: bool,
    report: Option<String>,
    tick: u128,
}

impl View {
    pub fn new(
        window: HWND,
        status: Arc<Mutex<Status>>,
        events: Sender<Event>,
        layout: Layout,
    ) -> Result<Self> {
        let fade_window = layout != Layout::Window;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let _ = BufferedPaintInit();
        }
        let resources = Resources::new()?;
        let _ = dwm(window, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND);
        let _ = dwm(window, DWMWA_TRANSITIONS_FORCEDISABLED, &BOOL(1));
        let backdrop = dwm(window, DWMWA_SYSTEMBACKDROP_TYPE, &DWMSBT_NONE).is_ok();
        let settings = UISettings::new().ok();
        let look = Look::current(settings.as_ref(), backdrop);
        let mut view = Self {
            window,
            status,
            events,
            resources,
            settings,
            backdrop,
            acrylic: None,
            fade_window,
            layout,
            look,
            target: None,
            images: Images::default(),
            scene: None,
            scale: 1.0,
            ui: Ui::default(),
            drag: None,
            volume_drag: None,
            volume_sent: None,
            volume_sent_at: None,
            wheel_rest: 0,
            opacity: if fade_window { 0.0 } else { 1.0 },
            tracking: false,
            report: None,
            tick: 0,
        };
        view.apply(look);
        Ok(view)
    }

    pub fn restyle(&mut self) {
        self.apply(Look::current(self.settings.as_ref(), self.backdrop));
    }

    pub fn layered(&self) -> bool {
        self.fade_window && self.acrylic == Some(false)
    }

    fn apply(&mut self, look: Look) {
        self.look = look;
        let window = self.window;
        let _ = dwm(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &BOOL::from(!look.light),
        );
        let border = COLORREF(bgr(Palette::new(&look).border));
        let _ = dwm(window, DWMWA_BORDER_COLOR, &border);
        if self.acrylic == Some(look.acrylic) {
            self.invalidate();
            return;
        }
        self.acrylic = Some(look.acrylic);
        unsafe {
            let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
            let layered = WS_EX_LAYERED.0 as isize;
            let style = if self.layered() {
                style | layered
            } else {
                style & !layered
            };
            SetWindowLongPtrW(window, GWL_EXSTYLE, style);
            if self.backdrop {
                let kind = if look.acrylic {
                    DWMSBT_TRANSIENTWINDOW
                } else {
                    DWMSBT_NONE
                };
                let _ = dwm(window, DWMWA_SYSTEMBACKDROP_TYPE, &kind);
            }
            let edge = if look.acrylic { -1 } else { 0 };
            let _ = DwmExtendFrameIntoClientArea(
                window,
                &MARGINS {
                    cxLeftWidth: edge,
                    cxRightWidth: edge,
                    cyTopHeight: edge,
                    cyBottomHeight: edge,
                },
            );
            let _ = SetWindowPos(
                window,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        self.set_opacity(self.opacity);
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity;
        if !self.layered() {
            self.invalidate();
            return;
        }
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        unsafe {
            let _ = SetLayeredWindowAttributes(self.window, COLORREF(0), alpha, LWA_ALPHA);
        }
    }

    pub fn rescale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            self.target = None;
            self.invalidate();
        }
    }

    pub fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.window), None, false);
        }
    }

    fn content(&self) -> Content {
        self.status
            .lock()
            .map(|status| Content::from(&status))
            .unwrap_or(Content::Offline(String::new()))
    }

    fn drags(&self) -> Drag {
        Drag {
            seek: self.drag,
            volume: self.volume_drag,
        }
    }

    fn frame(&self) -> Frame {
        match self.layout {
            Layout::Popup => return Frame::Natural,
            Layout::Osd => return Frame::Osd,
            Layout::Window => {}
        }
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(self.window, &mut client) };
        Frame::Fixed {
            width: client.right as f32 / self.scale,
            height: client.bottom as f32 / self.scale,
        }
    }

    pub fn layout(&mut self) -> Option<f32> {
        let content = self.content();
        let scene = self
            .resources
            .scene(&content, self.frame(), self.drags())
            .ok()?;
        let height = scene.height;
        self.scene = Some(scene);
        Some(height)
    }

    fn create_target(&self) -> Result<ID2D1DCRenderTarget> {
        let dpi = 96.0 * self.scale;
        unsafe {
            self.resources
                .factory
                .CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: dpi,
                    dpiY: dpi,
                    ..Default::default()
                })
        }
    }

    pub fn paint(&mut self, dc: HDC) {
        if self.target.is_none() {
            match self.create_target() {
                Ok(target) => {
                    self.target = Some(target);
                    self.images = Images::default();
                }
                Err(error) => {
                    log(format!("мини-плеер: не удалось начать отрисовку: {error}"));
                    return;
                }
            }
        }
        let Some(target) = self.target.clone() else {
            return;
        };
        let mut client = RECT::default();
        if unsafe { GetClientRect(self.window, &mut client) }.is_err() {
            return;
        }
        let parameters = BP_PAINTPARAMS {
            cbSize: size_of::<BP_PAINTPARAMS>() as u32,
            dwFlags: BPPF_ERASE,
            prcExclude: std::ptr::null(),
            pBlendFunction: std::ptr::null(),
        };
        let mut memory = HDC::default();
        let buffer = unsafe {
            BeginBufferedPaint(dc, &client, BPBF_TOPDOWNDIB, Some(&parameters), &mut memory)
        };
        let surface = if buffer != 0 { memory } else { dc };
        let finish = |update: bool| {
            if buffer != 0 {
                let _ = unsafe { EndBufferedPaint(buffer, update) };
            }
        };
        if let Err(error) = unsafe { target.BindDC(surface, &client) } {
            log(format!("мини-плеер: не удалось начать отрисовку: {error}"));
            finish(false);
            return;
        }
        let content = self.content();
        let Ok(scene) = self.resources.scene(&content, self.frame(), self.drags()) else {
            finish(false);
            return;
        };
        self.images
            .prepare(&self.resources, &target, self.scale, &scene, content.art());
        let ui = Ui {
            dragging: self.drag.is_some(),
            sliding: self.volume_drag.is_some(),
            ..self.ui
        };
        let opacity = if self.layered() { 1.0 } else { self.opacity };
        unsafe {
            target.SetTextAntialiasMode(if self.look.acrylic {
                D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE
            } else {
                D2D1_TEXT_ANTIALIAS_MODE_DEFAULT
            });
            target.BeginDraw();
        }
        let drawn = self.resources.draw(
            &target,
            &scene,
            &Palette::new(&self.look),
            &self.images,
            ui,
            opacity,
        );
        let ended = unsafe { target.EndDraw(None, None) };
        finish(true);
        if let Err(error) = drawn.and(ended) {
            if error.code() == D2DERR_RECREATE_TARGET {
                self.target = None;
                self.images = Images::default();
                self.invalidate();
            } else {
                log(format!("мини-плеер: ошибка отрисовки: {error}"));
            }
        }
        self.scene = Some(scene);
    }

    fn point(&self, lparam: LPARAM) -> (f32, f32) {
        let x = (lparam.0 & 0xFFFF) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        (x / self.scale, y / self.scale)
    }

    fn hit(&self, (x, y): (f32, f32)) -> Option<Part> {
        let scene = self.scene.as_ref()?;
        for control in &scene.controls {
            let c = control.center;
            if control.enabled && (x - c.X).abs() <= 20.0 && (y - c.Y).abs() <= 16.0 {
                return Some(control.part);
            }
        }
        if let Some(volume) = &scene.volume
            && volume.enabled
        {
            let c = volume.speaker;
            if (x - c.X).abs() <= 14.0 && (y - c.Y).abs() <= 12.0 {
                return Some(Part::Speaker);
            }
            let bar = volume.bar;
            let middle = (bar.top + bar.bottom) / 2.0;
            if x >= bar.left - 6.0 && x <= bar.right + 6.0 && (y - middle).abs() <= 8.0 {
                return Some(Part::Volume);
            }
        }
        let progress = scene.progress.as_ref()?;
        let bar = progress.bar;
        let middle = (bar.top + bar.bottom) / 2.0;
        let inside = x >= bar.left - 6.0 && x <= bar.right + 6.0 && (y - middle).abs() <= 8.0;
        (progress.seekable && inside).then_some(Part::Bar)
    }

    pub fn interactive(&self, x: i32, y: i32) -> bool {
        self.hit((x as f32 / self.scale, y as f32 / self.scale))
            .is_some()
    }

    fn fraction(&self, x: f32) -> Option<f32> {
        let bar = self.scene.as_ref()?.progress.as_ref()?.bar;
        Some(((x - bar.left) / (bar.right - bar.left)).clamp(0.0, 1.0))
    }

    fn volume_fraction(&self, x: f32) -> Option<f32> {
        let bar = self.scene.as_ref()?.volume.as_ref()?.bar;
        Some(((x - bar.left) / (bar.right - bar.left)).clamp(0.0, 1.0))
    }

    fn slide(&mut self, last: bool) {
        let Some(fraction) = self.volume_drag else {
            return;
        };
        if !last
            && self
                .volume_sent_at
                .is_some_and(|at| at.elapsed() < SLIDE_EVERY)
        {
            return;
        }
        let max = self
            .status
            .lock()
            .ok()
            .and_then(|status| status.player.as_ref().map(|(player, _)| player.volume_max))
            .unwrap_or(0);
        if max == 0 {
            return;
        }
        let level = (fraction * max as f32).round() as u32;
        if self.volume_sent != Some(level) {
            self.volume_sent = Some(level);
            self.volume_sent_at = Some(Instant::now());
            change_volume(&self.status, &self.events, VolumeChange::Set(level));
        }
    }

    pub fn wheel(&mut self, delta: i16) {
        let controllable = self
            .status
            .lock()
            .map(|status| volume_controllable(&status))
            .unwrap_or(false);
        if !controllable {
            return;
        }
        self.wheel_rest += delta as i32;
        while self.wheel_rest.abs() >= 120 {
            let change = if self.wheel_rest > 0 {
                VolumeChange::Up
            } else {
                VolumeChange::Down
            };
            self.wheel_rest -= 120 * self.wheel_rest.signum();
            change_volume(&self.status, &self.events, change);
        }
        self.invalidate();
    }

    pub fn busy(&self) -> bool {
        self.ui.pressed.is_some() || self.drag.is_some() || self.volume_drag.is_some()
    }

    pub fn reset(&mut self) {
        let captured = self.busy();
        self.ui = Ui::default();
        self.drag = None;
        self.volume_drag = None;
        self.volume_sent = None;
        if captured {
            unsafe {
                let _ = ReleaseCapture();
            }
        }
        self.invalidate();
    }

    pub fn clear_hover(&mut self) {
        if !self.busy() && self.ui.hover.is_some() {
            self.ui.hover = None;
            self.invalidate();
        }
    }

    pub fn mouse_move(&mut self, lparam: LPARAM) {
        if !self.tracking {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.window,
                dwHoverTime: 0,
            };
            self.tracking = unsafe { TrackMouseEvent(&mut track) }.is_ok();
        }
        let point = self.point(lparam);
        if self.drag.is_some() {
            self.drag = self.fraction(point.0);
            self.invalidate();
            return;
        }
        if self.volume_drag.is_some() {
            self.volume_drag = self.volume_fraction(point.0);
            self.slide(false);
            self.invalidate();
            return;
        }
        let hover = self.hit(point);
        if hover != self.ui.hover {
            self.ui.hover = hover;
            self.invalidate();
        }
    }

    pub fn mouse_leave(&mut self) {
        self.tracking = false;
        self.clear_hover();
    }

    pub fn mouse_down(&mut self, lparam: LPARAM) {
        let point = self.point(lparam);
        match self.hit(point) {
            Some(Part::Bar) => self.drag = self.fraction(point.0),
            Some(Part::Volume) => {
                self.volume_sent = None;
                self.volume_sent_at = None;
                self.volume_drag = self.volume_fraction(point.0);
                self.slide(false);
            }
            Some(part) => self.ui.pressed = Some(part),
            None => return,
        }
        self.ui.hover = self.hit(point);
        unsafe { SetCapture(self.window) };
        self.invalidate();
    }

    pub fn mouse_up(&mut self, lparam: LPARAM) {
        let point = self.point(lparam);
        let pressed = self.ui.pressed.take();
        let drag = self.drag.take();
        if self.volume_drag.is_some() {
            self.volume_drag = self.volume_fraction(point.0);
            self.slide(true);
        }
        let sliding = self.volume_drag.take().is_some();
        self.volume_sent = None;
        self.volume_sent_at = None;
        if pressed.is_none() && drag.is_none() && !sliding {
            return;
        }
        unsafe {
            let _ = ReleaseCapture();
        }
        if let Some(fraction) = drag {
            self.seek(fraction);
        } else if let Some(part) = pressed
            && self.hit(point) == Some(part)
        {
            self.press(part);
        }
        self.ui.hover = self.hit(point);
        self.invalidate();
    }

    pub fn lost_capture(&mut self) {
        if self.busy() {
            self.ui.pressed = None;
            self.drag = None;
            self.volume_drag = None;
            self.volume_sent = None;
            self.invalidate();
        }
    }

    fn press(&self, part: Part) {
        let playing = self
            .status
            .lock()
            .ok()
            .and_then(|status| status.player.as_ref().map(|(player, _)| player.playing))
            .unwrap_or(false);
        let button = match part {
            Part::Previous => Button::Previous,
            Part::Next => Button::Next,
            Part::Toggle if playing => Button::Pause,
            Part::Toggle => Button::Play,
            Part::Speaker => {
                change_volume(&self.status, &self.events, VolumeChange::Mute);
                return;
            }
            Part::Bar | Part::Volume => return,
        };
        let _ = self.events.send(Event::Button(button));
    }

    fn seek(&self, fraction: f32) {
        let Ok(mut status) = self.status.lock() else {
            return;
        };
        let Some((player, received)) = status.player.as_mut() else {
            return;
        };
        let position = Duration::from_millis(player.duration_ms).mul_f32(fraction);
        player.position_ms = position.as_millis() as u64;
        *received = Instant::now();
        let _ = self.events.send(Event::Seek(position));
    }

    pub fn set_iconic(&self, enabled: bool) {
        let value = BOOL::from(enabled);
        let _ = dwm(self.window, DWMWA_FORCE_ICONIC_REPRESENTATION, &value);
        let _ = dwm(self.window, DWMWA_HAS_ICONIC_BITMAP, &value);
    }

    pub fn invalidate_iconic(&self) {
        unsafe {
            let _ = DwmInvalidateIconicBitmaps(self.window);
        }
    }

    pub fn send_thumbnail(&mut self, max_width: i32, max_height: i32) {
        let content = self.content();
        let (box_width, box_height) = (max_width as f32, max_height as f32);
        let scale = (box_height / THUMB_HEIGHT).min(box_width / THUMB_MIN_WIDTH);
        let frame = Frame::Preview {
            width: box_width / scale,
            height: box_height / scale,
        };
        let Ok(scene) = self.resources.scene(&content, frame, Drag::default()) else {
            return;
        };
        let bitmap = match self.snapshot(&content, &scene, scale) {
            Ok(bitmap) => bitmap,
            Err(error) => {
                log(format!("мини-плеер: не удалось нарисовать превью: {error}"));
                return;
            }
        };
        let accepted = unsafe {
            let result = DwmSetIconicThumbnail(self.window, bitmap, 0);
            let _ = DeleteObject(bitmap.into());
            result
        };
        if let Err(error) = accepted {
            let report = format!("превью {max_width}×{max_height} не принято: {error}");
            if self.report.as_deref() != Some(report.as_str()) {
                log(&report);
                self.report = Some(report);
            }
        }
    }

    pub fn send_preview(&self) {
        let content = self.content();
        let Ok(scene) = self
            .resources
            .scene(&content, self.frame(), Drag::default())
        else {
            return;
        };
        match self.snapshot(&content, &scene, self.scale) {
            Ok(bitmap) => unsafe {
                let _ = DwmSetIconicLivePreviewBitmap(self.window, bitmap, None, 0);
                let _ = DeleteObject(bitmap.into());
            },
            Err(error) => log(format!("мини-плеер: не удалось нарисовать превью: {error}")),
        }
    }

    fn snapshot(&self, content: &Content, scene: &Scene, scale: f32) -> Result<HBITMAP> {
        let width = (scene.width * scale + 0.01).floor().max(1.0) as i32;
        let height = (scene.height * scale + 0.01).floor().max(1.0) as i32;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let dc = CreateCompatibleDC(None);
            let previous = SelectObject(dc, bitmap.into());
            let drawn = self.draw_into(dc, (width, height), content, scene, scale);
            SelectObject(dc, previous);
            let _ = DeleteDC(dc);
            match drawn {
                Ok(()) => Ok(bitmap),
                Err(error) => {
                    let _ = DeleteObject(bitmap.into());
                    Err(error)
                }
            }
        }
    }

    fn draw_into(
        &self,
        dc: HDC,
        (width, height): (i32, i32),
        content: &Content,
        scene: &Scene,
        scale: f32,
    ) -> Result<()> {
        let dpi = 96.0 * scale;
        unsafe {
            let target =
                self.resources
                    .factory
                    .CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: dpi,
                        dpiY: dpi,
                        ..Default::default()
                    })?;
            let frame = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            target.BindDC(dc, &frame)?;
            let mut images = Images::default();
            images.prepare(&self.resources, &target, scale, scene, content.art());
            let look = Look {
                acrylic: false,
                ..self.look
            };
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.BeginDraw();
            let drawn = self.resources.draw(
                &target,
                scene,
                &Palette::new(&look),
                &images,
                Ui::default(),
                1.0,
            );
            let ended = target.EndDraw(None, None);
            drawn.and(ended)
        }
    }

    pub fn title(&self) -> String {
        match self.content() {
            Content::Playing(playing) if !playing.artist.is_empty() => {
                format!("{} — {}", playing.artist, playing.title)
            }
            Content::Playing(playing) => playing.title,
            _ => "Phone Media".to_string(),
        }
    }

    pub fn transport(&self) -> Transport {
        match self.content() {
            Content::Playing(playing) => Transport {
                playing: playing.playing,
                previous: playing.can_previous,
                toggle: playing.can_toggle,
                next: playing.can_next,
            },
            _ => Transport::default(),
        }
    }

    pub fn command(&self, id: u32) {
        let part = match id {
            COMMAND_PREVIOUS => Part::Previous,
            COMMAND_TOGGLE => Part::Toggle,
            COMMAND_NEXT => Part::Next,
            _ => return,
        };
        self.press(part);
    }

    pub fn glyph(&self, glyph: Glyph, size: i32) -> Result<HICON> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let ink = if self.look.light { 0x000000 } else { 0xffffff };
        let shape = match glyph {
            Glyph::Previous => &self.resources.previous,
            Glyph::Play => &self.resources.play,
            Glyph::Pause => &self.resources.pause,
            Glyph::Next => &self.resources.next,
        };
        unsafe {
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let dc = CreateCompatibleDC(None);
            let previous = SelectObject(dc, bitmap.into());
            let drawn = (|| -> Result<()> {
                let target = self.resources.factory.CreateDCRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        ..Default::default()
                    },
                )?;
                let frame = RECT {
                    left: 0,
                    top: 0,
                    right: size,
                    bottom: size,
                };
                target.BindDC(dc, &frame)?;
                let brush = target.CreateSolidColorBrush(&color(ink, 1.0), None)?;
                let half = size as f32 / 2.0;
                let scale = size as f32 / 20.0;
                target.BeginDraw();
                target.Clear(Some(&color(0, 0.0)));
                target.SetTransform(
                    &(Matrix3x2::scale(scale, scale) * Matrix3x2::translation(half, half)),
                );
                target.FillGeometry(shape, &brush, None);
                target.DrawGeometry(shape, &brush, 2.0, &self.resources.round);
                target.EndDraw(None, None)
            })();
            SelectObject(dc, previous);
            let _ = DeleteDC(dc);
            let _ = GdiFlush();
            if let Err(error) = drawn {
                let _ = DeleteObject(bitmap.into());
                return Err(error);
            }
            let pixels =
                std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
            for pixel in pixels.as_chunks_mut::<4>().0 {
                let alpha = pixel[3] as u32;
                if alpha > 0 && alpha < 255 {
                    for channel in &mut pixel[..3] {
                        *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
                    }
                }
            }
            let stride = (size as usize).div_ceil(16) * 2;
            let blank = vec![0u8; stride * size as usize];
            let mask = CreateBitmap(size, size, 1, 1, Some(blank.as_ptr().cast()));
            let icon = CreateIconIndirect(&ICONINFO {
                fIcon: BOOL(1),
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: bitmap,
            });
            let _ = DeleteObject(mask.into());
            let _ = DeleteObject(bitmap.into());
            icon
        }
    }

    pub fn tick(&mut self) -> bool {
        let tick = self.status.lock().ok().and_then(|status| {
            status
                .player
                .as_ref()
                .filter(|(player, _)| player.playing)
                .map(|(_, received)| received.elapsed().as_millis() / 250)
        });
        if let Some(tick) = tick
            && tick != self.tick
        {
            self.tick = tick;
            self.invalidate();
            return true;
        }
        false
    }
}

fn font_family(write: &IDWriteFactory) -> HSTRING {
    let mut collection = None;
    unsafe {
        if write
            .GetSystemFontCollection(&mut collection, false)
            .is_ok()
            && let Some(collection) = collection
        {
            for name in ["Segoe UI Variable Text", "Segoe UI"] {
                let (mut index, mut exists) = (0, BOOL(0));
                if collection
                    .FindFamilyName(&HSTRING::from(name), &mut index, &mut exists)
                    .is_ok()
                    && exists.as_bool()
                {
                    return HSTRING::from(name);
                }
            }
        }
    }
    HSTRING::from("Segoe UI")
}

fn personalize(name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            &HSTRING::from(name),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as _),
            Some(&mut size),
        )
    };
    result.is_ok().then_some(value)
}

fn dwm<T>(window: HWND, attribute: DWMWINDOWATTRIBUTE, value: &T) -> Result<()> {
    unsafe {
        DwmSetWindowAttribute(
            window,
            attribute,
            value as *const T as _,
            size_of::<T>() as u32,
        )
    }
}

fn clock(time: Duration, long: bool) -> String {
    let seconds = time.as_secs();
    if long {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a: alpha,
    }
}

fn bgr(rgb: u32) -> u32 {
    ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
}

fn v(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

fn rounded(rect: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect,
        radiusX: radius,
        radiusY: radius,
    }
}

fn circle(x: f32, y: f32, radius: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: v(x, y),
        radiusX: radius,
        radiusY: radius,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::GENERIC_WRITE;
    use windows::Win32::Graphics::Imaging::{GUID_ContainerFormatPng, WICBitmapEncoderNoCache};

    #[test]
    fn height_follows_cover() {
        assert_eq!(fit_height(720.0), 16.0 + 344.0 + 100.0);
        assert_eq!(fit_height(300.0), 16.0 + 268.0 + 12.0 + 58.0 + 8.0 + 100.0);
        assert_eq!(fit_height(1200.0), 16.0 + 480.0 + 100.0);
        assert_eq!(tallest(), 16.0 + 480.0 + 12.0 + 58.0 + 8.0 + 100.0);
    }

    #[test]
    #[ignore]
    fn preview() -> Result<()> {
        let folder = std::env::var("PHONE_MEDIA_PREVIEW").unwrap_or_else(|_| ".".to_string());
        let art: Option<Arc<[u8]>> = std::env::var("PHONE_MEDIA_PREVIEW_ART")
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .map(Arc::from);
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let resources = Resources::new()?;
        let playing = Playing {
            title: "Crucifix X".to_string(),
            artist: "Ave Mujica".to_string(),
            source: "Яндекс Музыка · CROSBIC-S25".to_string(),
            playing: true,
            position: Duration::from_secs(92),
            duration: Duration::from_secs(236),
            can_previous: true,
            can_next: true,
            can_toggle: true,
            can_seek: true,
            art,
            volume: Some(Level {
                level: 9,
                max: 15,
                muted: false,
                fixed: false,
            }),
        };
        let fine = Playing {
            volume: Some(Level {
                level: 104,
                max: 150,
                muted: false,
                fixed: false,
            }),
            ..playing.clone()
        };
        let paused = Playing {
            title: "Очень длинное название трека, которое точно не поместится в одну строку"
                .to_string(),
            artist: String::new(),
            source: "Радио · CROSBIC-S25".to_string(),
            playing: false,
            position: Duration::from_secs(4000),
            duration: Duration::from_secs(5400),
            can_previous: false,
            can_seek: false,
            art: None,
            volume: Some(Level {
                level: 4,
                max: 15,
                muted: true,
                fixed: false,
            }),
            ..playing.clone()
        };
        let hover = Ui {
            hover: Some(Part::Toggle),
            ..Ui::default()
        };
        let natural = Frame::Natural;
        let fixed = |width: f32, height: f32| Frame::Fixed { width, height };
        let cases = [
            (
                "dark-playing",
                false,
                Content::Playing(playing.clone()),
                Ui::default(),
            ),
            (
                "light-playing",
                true,
                Content::Playing(playing.clone()),
                hover,
            ),
            (
                "dark-paused",
                false,
                Content::Playing(paused.clone()),
                hover,
            ),
            (
                "light-paused",
                true,
                Content::Playing(paused),
                Ui::default(),
            ),
            (
                "dark-offline",
                false,
                Content::Offline("Ищу телефон…".to_string()),
                Ui::default(),
            ),
            (
                "light-pairing",
                true,
                Content::Offline("Жду новый телефон: откройте на нём Phone Media".to_string()),
                Ui::default(),
            ),
            (
                "dark-idle",
                false,
                Content::Idle("CROSBIC-S25".to_string()),
                Ui::default(),
            ),
        ]
        .map(|(name, light, content, ui)| (name, light, content, ui, natural));
        let windows = [
            (
                "window-wide",
                false,
                Content::Playing(playing.clone()),
                Ui::default(),
                fixed(720.0, fit_height(720.0)),
            ),
            (
                "window-tall",
                false,
                Content::Playing(playing.clone()),
                Ui::default(),
                fixed(300.0, fit_height(300.0)),
            ),
            (
                "window-between",
                true,
                Content::Playing(playing.clone()),
                Ui::default(),
                fixed(400.0, 330.0),
            ),
            (
                "window-small",
                true,
                Content::Playing(playing.clone()),
                Ui::default(),
                fixed(260.0, 150.0),
            ),
            (
                "fine",
                false,
                Content::Playing(fine),
                Ui::default(),
                Frame::Natural,
            ),
            (
                "osd",
                true,
                Content::Playing(playing.clone()),
                Ui::default(),
                Frame::Osd,
            ),
            (
                "thumbnail",
                false,
                Content::Playing(playing),
                Ui::default(),
                Frame::Preview {
                    width: 250.0,
                    height: 100.0,
                },
            ),
            (
                "window-offline",
                false,
                Content::Offline("Ищу телефон…".to_string()),
                Ui::default(),
                fixed(340.0, 170.0),
            ),
        ];
        let scale = 1.5;
        let look = |light: bool| Look {
            light,
            acrylic: false,
            accent: if light { 0x009faa } else { 0x29f7ff },
            tint: None,
            animated: true,
        };
        for (name, light, content, ui, frame) in cases.into_iter().chain(windows) {
            let scene = resources.scene(&content, frame, Drag::default())?;
            let width = (scene.width * scale).round() as u32;
            let height = (scene.height * scale).round() as u32;
            unsafe {
                let bitmap = resources.wic.CreateBitmap(
                    width,
                    height,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnLoad,
                )?;
                let target = resources.factory.CreateWicBitmapRenderTarget(
                    &bitmap,
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0 * scale,
                        dpiY: 96.0 * scale,
                        ..Default::default()
                    },
                )?;
                let mut images = Images::default();
                images.prepare(&resources, &target, scale, &scene, content.art());
                target.BeginDraw();
                resources.draw(
                    &target,
                    &scene,
                    &Palette::new(&look(light)),
                    &images,
                    ui,
                    1.0,
                )?;
                target.EndDraw(None, None)?;
                save_png(&resources.wic, &bitmap, &format!("{folder}/{name}.png"))?;
            }
        }
        Ok(())
    }

    fn save_png(wic: &IWICImagingFactory, bitmap: &IWICBitmap, path: &str) -> Result<()> {
        unsafe {
            let stream = wic.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path), GENERIC_WRITE.0)?;
            let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.ok_or_else(windows::core::Error::empty)?;
            frame.Initialize(None)?;
            frame.WriteSource(bitmap, std::ptr::null())?;
            frame.Commit()?;
            encoder.Commit()
        }
    }
}
