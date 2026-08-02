//! Icon / IconButton — 基于内置图标字体的矢量图标与工具栏图标按钮
//!
//! 项目随附 Google Material Icons（OFL 许可）字体 `assets/MaterialIcons-Regular.ttf`，
//! 图标直接以文本字形渲染，可缩放、可着色（含 hover/pressed 变色）。
//!
//! 字体通过 `include_bytes!` 在编译期内嵌到二进制中，首次构建图标时
//! 自动注册到排版引擎，**无需任何手动注册**。
//!
//! ```ignore
//! IconButton::new(IconName::Add).on_click(|| add())
//! Icon::new(IconName::Search, 18.0).color(Color::GRAY)
//! ```
//!
//! 码点与 `assets/MaterialIcons-Regular.codepoints` 一一对应。

use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::TextStyle;
use crate::widget::button::{Button, ButtonVariant};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;
use std::str::FromStr;

/// 内嵌的 Material Icons 字体字节（编译期 `include_bytes!`，无需运行时文件）。
pub static ICON_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/MaterialIcons-Regular.ttf"
));

thread_local! {
    static ICON_FONT_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 确保当前线程已注册图标字体（首次调用时注册一次，幂等）。
/// 排版引擎的字体上下文是线程局部的，因此按线程惰性注册。
pub fn ensure_icon_font_registered() {
    ICON_FONT_REGISTERED.with(|r| {
        if !r.get() {
            crate::text::register_font_bytes(ICON_FONT_BYTES.to_vec());
            r.set(true);
        }
    });
}

/// 内置图标名称（Material Icons，OFL 许可）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub enum IconName {
    Add,
    Remove,
    Close,
    Check,
    Done,
    Search,
    Settings,
    Delete,
    DeleteForever,
    Menu,
    MoreVert,
    ArrowBack,
    ArrowForward,
    ArrowUp,
    ArrowDown,
    ChevronRight,
    KeyboardArrowRight,
    KeyboardArrowLeft,
    KeyboardArrowUp,
    KeyboardArrowDown,
    ExpandMore,
    ExpandLess,
    Home,
    Person,
    AccountCircle,
    Group,
    PersonAdd,
    Folder,
    Star,
    StarBorder,
    Favorite,
    FavoriteBorder,
    ThumbUp,
    ThumbDown,
    Save,
    SaveAlt,
    Edit,
    Create,
    PlayArrow,
    Pause,
    Stop,
    Refresh,
    Sync,
    Undo,
    Redo,
    Mail,
    Send,
    Phone,
    Info,
    Warning,
    Error,
    CheckCircle,
    Help,
    Lock,
    ShoppingCart,
    ShoppingBasket,
    CreditCard,
    Payment,
    Notifications,
    Visibility,
    List,
    Download,
    Upload,
    OpenInNew,
    ExitToApp,
    ZoomIn,
    ZoomOut,
    DateRange,
    Event,
    Schedule,
    Alarm,
    Build,
    Camera,
    Cloud,
    Code,
    Dashboard,
    Flag,
    Flight,
    GetApp,
    Grade,
    Headset,
    History,
    Image,
    InsertChart,
    Language,
    Lightbulb,
    LocationOn,
    Mic,
    Movie,
    MusicNote,
    Photo,
    Place,
    PowerSettingsNew,
    Public,
    Receipt,
    Restore,
    Room,
    School,
    Share,
    Sort,
    TextFields,
    Tune,
    VerifiedUser,
    Videocam,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    VolumeOff,
    WbSunny,
    Work,
    ZoomOutMap,
    Face,
    Feedback,
    FileCopy,
    Timer,
    Toc,
    Chat,
    Comment,
    Contacts,
    DragIndicator,
    RadioButtonChecked,
    SkipNext,
    SkipPrevious,
    ThumbsUpDown,
    Bookmark,
    AttachFile,
    Fullscreen,
    Print,
    ContentCopy,
    FilterList,
    CloseFullscreen,
    RotateLeft,
    RotateRight,
    Rotate90DegreesCcw,
    Rotate90DegreesCw,
    Flip,
    FlipToBack,
    FlipToFront,
    // 导航
    ArrowDropDown,
    ArrowDropUp,
    ArrowCircleDown,
    ArrowCircleUp,
    ArrowCircleLeft,
    ArrowCircleRight,
    FirstPage,
    LastPage,
    SubdirectoryArrowLeft,
    SubdirectoryArrowRight,
    // 切换/选择
    CheckBox,
    CheckBoxOutlineBlank,
    RadioButtonUnchecked,
    ToggleOff,
    ToggleOn,
    StarHalf,
    // 文本格式
    FormatBold,
    FormatItalic,
    FormatUnderlined,
    FormatStrikethrough,
    FormatAlignLeft,
    FormatAlignCenter,
    FormatAlignRight,
    FormatAlignJustify,
    FormatListBulleted,
    FormatListNumbered,
    FormatColorFill,
    FormatPaint,
    FormatQuote,
    FormatSize,
    // 媒体播放
    Shuffle,
    Repeat,
    RepeatOne,
    Loop,
    FastForward,
    FastRewind,
    PlaylistAdd,
    PlaylistPlay,
    QueueMusic,
    // 剪贴板
    ContentCut,
    ContentPaste,
    // 文件
    FolderOpen,
    FileDownload,
    FileUpload,
    CreateNewFolder,
    // 动作
    MoreHoriz,
    Cached,
    AutoRenew,
    RestartAlt,
    SwapHoriz,
    SwapVert,
    CompareArrows,
    Palette,
    ColorLens,
    Layers,
    Apps,
    Widgets,
    // 设备状态
    Wifi,
    WifiOff,
    Bluetooth,
    BluetoothConnected,
    BluetoothDisabled,
    BatteryFull,
    BatteryChargingFull,
    GpsFixed,
    GpsOff,
    Storage,
    // 通信
    ChatBubble,
    ChatBubbleOutline,
    Markunread,
    AlternateEmail,
    Call,
    CallEnd,
    RingVolume,
    Dialpad,
    SpeakerPhone,
    // 硬件
    Keyboard,
    KeyboardCapslock,
    KeyboardHide,
    KeyboardReturn,
    KeyboardTab,
    Mouse,
    Headphones,
    HeadsetMic,
    Laptop,
    DesktopWindows,
    DesktopMac,
    Gamepad,
    Computer,
    // 社交
    GroupAdd,
    People,
    PeopleOutline,
    PersonOutline,
    PersonRemove,
    EmojiEmotions,
    Mood,
    MoodBad,
    // 地图/地点
    Map,
    Directions,
    DirectionsWalk,
    DirectionsRun,
    Restaurant,
    LocalCafe,
    LocalDining,
    Store,
    Hotel,
    LocalHospital,
    LocalParking,
    LocalPharmacy,
    LocalOffer,
    LocalGroceryStore,
    LocalMall,
    LocalShipping,
    LocalTaxi,
    LocalAtm,
    LocalPhone,
    LocalPizza,
    LocalPostOffice,
    LocalPrintShop,
    Navigation,
    NearMe,
    RateReview,
    DepartureBoard,
    // 图像
    AddAPhoto,
    AddPhotoAlternate,
    CameraAlt,
    Collections,
    Panorama,
    PhotoCamera,
    PhotoLibrary,
    PhotoAlbum,
    PhotoFilter,
    Nature,
    NaturePeople,
    Landscape,
    Looks,
    Portrait,
    WbCloudy,
    WbTwilight,
}

impl IconName {
    /// 内置图标字体的 family 名（`register_font_file` 返回的注册名）。
    pub const FONT_FAMILY: &'static str = "Material Icons";

    /// 图标对应的 Unicode 码点（与 `assets/MaterialIcons-Regular.codepoints` 一致）。
    pub fn codepoint(self) -> u32 {
        use IconName::*;
        match self {
            Add => 0xe145,
            Remove => 0xe15b,
            Close => 0xe5cd,
            Check => 0xe5ca,
            Done => 0xe876,
            Search => 0xe8b6,
            Settings => 0xe8b8,
            Delete => 0xe872,
            DeleteForever => 0xe92b,
            Menu => 0xe5d2,
            MoreVert => 0xe5d4,
            ArrowBack => 0xe5c4,
            ArrowForward => 0xe5c8,
            ArrowUp => 0xe5d8,
            ArrowDown => 0xe5db,
            ChevronRight => 0xe5cc,
            KeyboardArrowRight => 0xe315,
            KeyboardArrowLeft => 0xe314,
            KeyboardArrowUp => 0xe316,
            KeyboardArrowDown => 0xe313,
            ExpandMore => 0xe5cf,
            ExpandLess => 0xe5ce,
            Home => 0xe88a,
            Person => 0xe7fd,
            AccountCircle => 0xe853,
            Group => 0xe7ef,
            PersonAdd => 0xe7fe,
            Folder => 0xe2c7,
            Star => 0xe838,
            StarBorder => 0xe83a,
            Favorite => 0xe87d,
            FavoriteBorder => 0xe87e,
            ThumbUp => 0xe8dc,
            ThumbDown => 0xe8db,
            Save => 0xe161,
            SaveAlt => 0xe171,
            Edit => 0xe3c9,
            Create => 0xe150,
            PlayArrow => 0xe037,
            Pause => 0xe034,
            Stop => 0xe047,
            Refresh => 0xe5d5,
            Sync => 0xe627,
            Undo => 0xe166,
            Redo => 0xe15a,
            Mail => 0xe158,
            Send => 0xe163,
            Phone => 0xe0cd,
            Info => 0xe88e,
            Warning => 0xe002,
            Error => 0xe000,
            CheckCircle => 0xe86c,
            Help => 0xe887,
            Lock => 0xe897,
            ShoppingCart => 0xe8cc,
            ShoppingBasket => 0xe8cb,
            CreditCard => 0xe870,
            Payment => 0xe8a1,
            Notifications => 0xe7f4,
            Visibility => 0xe8f4,
            List => 0xe896,
            Download => 0xf090,
            Upload => 0xf09b,
            OpenInNew => 0xe89e,
            ExitToApp => 0xe879,
            ZoomIn => 0xe8ff,
            ZoomOut => 0xe900,
            DateRange => 0xe916,
            Event => 0xe878,
            Schedule => 0xe8b5,
            Alarm => 0xe855,
            Build => 0xe869,
            Camera => 0xe3af,
            Cloud => 0xe2bd,
            Code => 0xe86f,
            Dashboard => 0xe871,
            Flag => 0xe153,
            Flight => 0xe539,
            GetApp => 0xe884,
            Grade => 0xe885,
            Headset => 0xe310,
            History => 0xe889,
            Image => 0xe3f4,
            InsertChart => 0xe24b,
            Language => 0xe894,
            Lightbulb => 0xe0f0,
            LocationOn => 0xe0c8,
            Mic => 0xe029,
            Movie => 0xe02c,
            MusicNote => 0xe405,
            Photo => 0xe410,
            Place => 0xe55f,
            PowerSettingsNew => 0xe8ac,
            Public => 0xe80b,
            Receipt => 0xe8b0,
            Restore => 0xe8b3,
            Room => 0xe8b4,
            School => 0xe80c,
            Share => 0xe80d,
            Sort => 0xe164,
            TextFields => 0xe262,
            Tune => 0xe429,
            VerifiedUser => 0xe8e8,
            Videocam => 0xe04b,
            VolumeUp => 0xe050,
            VolumeDown => 0xe04d,
            VolumeMute => 0xe04e,
            VolumeOff => 0xe04f,
            WbSunny => 0xe430,
            Work => 0xe8f9,
            ZoomOutMap => 0xe56b,
            Face => 0xe87c,
            Feedback => 0xe87f,
            FileCopy => 0xe173,
            Timer => 0xe425,
            Toc => 0xe8de,
            Chat => 0xe0b7,
            Comment => 0xe0b9,
            Contacts => 0xe0ba,
            DragIndicator => 0xe945,
            RadioButtonChecked => 0xe837,
            SkipNext => 0xe044,
            SkipPrevious => 0xe045,
            ThumbsUpDown => 0xe8dd,
            Bookmark => 0xe866,
            AttachFile => 0xe226,
            Fullscreen => 0xe5d0,
            Print => 0xe8ad,
            ContentCopy => 0xe14d,
            FilterList => 0xe152,
            CloseFullscreen => 0xf1cf,
            RotateLeft => 0xe419,
            RotateRight => 0xe41a,
            Rotate90DegreesCcw => 0xe418,
            Rotate90DegreesCw => 0xeaab,
            Flip => 0xe3e8,
            FlipToBack => 0xe882,
            FlipToFront => 0xe883,
            // 导航
            ArrowDropDown => 0xe5c5,
            ArrowDropUp => 0xe5c7,
            ArrowCircleDown => 0xf181,
            ArrowCircleUp => 0xf182,
            ArrowCircleLeft => 0xeaa7,
            ArrowCircleRight => 0xeaaa,
            FirstPage => 0xe5dc,
            LastPage => 0xe5dd,
            SubdirectoryArrowLeft => 0xe5d9,
            SubdirectoryArrowRight => 0xe5da,
            // 切换/选择
            CheckBox => 0xe834,
            CheckBoxOutlineBlank => 0xe835,
            RadioButtonUnchecked => 0xe836,
            ToggleOff => 0xe9f5,
            ToggleOn => 0xe9f6,
            StarHalf => 0xe839,
            // 文本格式
            FormatBold => 0xe238,
            FormatItalic => 0xe23f,
            FormatUnderlined => 0xe249,
            FormatStrikethrough => 0xe246,
            FormatAlignLeft => 0xe236,
            FormatAlignCenter => 0xe234,
            FormatAlignRight => 0xe237,
            FormatAlignJustify => 0xe235,
            FormatListBulleted => 0xe241,
            FormatListNumbered => 0xe242,
            FormatColorFill => 0xe23a,
            FormatPaint => 0xe243,
            FormatQuote => 0xe244,
            FormatSize => 0xe245,
            // 媒体播放
            Shuffle => 0xe043,
            Repeat => 0xe040,
            RepeatOne => 0xe041,
            Loop => 0xe028,
            FastForward => 0xe01f,
            FastRewind => 0xe020,
            PlaylistAdd => 0xe03b,
            PlaylistPlay => 0xe05f,
            QueueMusic => 0xe03d,
            // 剪贴板
            ContentCut => 0xe14e,
            ContentPaste => 0xe14f,
            // 文件
            FolderOpen => 0xe2c8,
            FileDownload => 0xe2c4,
            FileUpload => 0xe2c6,
            CreateNewFolder => 0xe2cc,
            // 动作
            MoreHoriz => 0xe5d3,
            Cached => 0xe86a,
            AutoRenew => 0xe863,
            RestartAlt => 0xf053,
            SwapHoriz => 0xe8d4,
            SwapVert => 0xe8d5,
            CompareArrows => 0xe915,
            Palette => 0xe40a,
            ColorLens => 0xe3b7,
            Layers => 0xe53b,
            Apps => 0xe5c3,
            Widgets => 0xe1bd,
            // 设备状态
            Wifi => 0xe63e,
            WifiOff => 0xe648,
            Bluetooth => 0xe1a7,
            BluetoothConnected => 0xe1a8,
            BluetoothDisabled => 0xe1a9,
            BatteryFull => 0xe1a4,
            BatteryChargingFull => 0xe1a3,
            GpsFixed => 0xe1b3,
            GpsOff => 0xe1b5,
            Storage => 0xe1db,
            // 通信
            ChatBubble => 0xe0ca,
            ChatBubbleOutline => 0xe0cb,
            Markunread => 0xe159,
            AlternateEmail => 0xe0e6,
            Call => 0xe0b0,
            CallEnd => 0xe0b1,
            RingVolume => 0xe0d1,
            Dialpad => 0xe0bc,
            SpeakerPhone => 0xe0d2,
            // 硬件
            Keyboard => 0xe312,
            KeyboardCapslock => 0xe318,
            KeyboardHide => 0xe31a,
            KeyboardReturn => 0xe31b,
            KeyboardTab => 0xe31c,
            Mouse => 0xe323,
            Headphones => 0xf01f,
            HeadsetMic => 0xe311,
            Laptop => 0xe31e,
            DesktopWindows => 0xe30c,
            DesktopMac => 0xe30b,
            Gamepad => 0xe30f,
            Computer => 0xe30a,
            // 社交
            GroupAdd => 0xe7f0,
            People => 0xe7fb,
            PeopleOutline => 0xe7fc,
            PersonOutline => 0xe7ff,
            PersonRemove => 0xef66,
            EmojiEmotions => 0xea22,
            Mood => 0xe7f2,
            MoodBad => 0xe7f3,
            // 地图/地点
            Map => 0xe55b,
            Directions => 0xe52e,
            DirectionsWalk => 0xe536,
            DirectionsRun => 0xe566,
            Restaurant => 0xe56c,
            LocalCafe => 0xe541,
            LocalDining => 0xe556,
            Store => 0xe8d1,
            Hotel => 0xe53a,
            LocalHospital => 0xe548,
            LocalParking => 0xe54f,
            LocalPharmacy => 0xe550,
            LocalOffer => 0xe54e,
            LocalGroceryStore => 0xe547,
            LocalMall => 0xe54c,
            LocalShipping => 0xe558,
            LocalTaxi => 0xe559,
            LocalAtm => 0xe53e,
            LocalPhone => 0xe551,
            LocalPizza => 0xe552,
            LocalPostOffice => 0xe554,
            LocalPrintShop => 0xe555,
            Navigation => 0xe55d,
            NearMe => 0xe569,
            RateReview => 0xe560,
            DepartureBoard => 0xe576,
            // 图像
            AddAPhoto => 0xe439,
            AddPhotoAlternate => 0xe43e,
            CameraAlt => 0xe3b0,
            Collections => 0xe3b6,
            Panorama => 0xe40b,
            PhotoCamera => 0xe412,
            PhotoLibrary => 0xe413,
            PhotoAlbum => 0xe411,
            PhotoFilter => 0xe43b,
            Nature => 0xe406,
            NaturePeople => 0xe407,
            Landscape => 0xe3f7,
            Looks => 0xe3fc,
            Portrait => 0xe416,
            WbCloudy => 0xe42d,
            WbTwilight => 0xe1c6,
        }
    }

    /// 图标字形（单个字符）。
    pub fn char(self) -> char {
        char::from_u32(self.codepoint()).expect("valid icon codepoint")
    }

    /// 返回图标在 `codepoints` 文件中的 snake_case 名称。
    pub fn name_str(self) -> &'static str {
        use IconName::*;
        match self {
            Add => "add",
            Remove => "remove",
            Close => "close",
            Check => "check",
            Done => "done",
            Search => "search",
            Settings => "settings",
            Delete => "delete",
            DeleteForever => "delete_forever",
            Menu => "menu",
            MoreVert => "more_vert",
            ArrowBack => "arrow_back",
            ArrowForward => "arrow_forward",
            ArrowUp => "arrow_up",
            ArrowDown => "arrow_down",
            ChevronRight => "chevron_right",
            KeyboardArrowRight => "keyboard_arrow_right",
            KeyboardArrowLeft => "keyboard_arrow_left",
            KeyboardArrowUp => "keyboard_arrow_up",
            KeyboardArrowDown => "keyboard_arrow_down",
            ExpandMore => "expand_more",
            ExpandLess => "expand_less",
            Home => "home",
            Person => "person",
            AccountCircle => "account_circle",
            Group => "group",
            PersonAdd => "person_add",
            Folder => "folder",
            Star => "star",
            StarBorder => "star_border",
            Favorite => "favorite",
            FavoriteBorder => "favorite_border",
            ThumbUp => "thumb_up",
            ThumbDown => "thumb_down",
            Save => "save",
            SaveAlt => "save_alt",
            Edit => "edit",
            Create => "create",
            PlayArrow => "play_arrow",
            Pause => "pause",
            Stop => "stop",
            Refresh => "refresh",
            Sync => "sync",
            Undo => "undo",
            Redo => "redo",
            Mail => "mail",
            Send => "send",
            Phone => "phone",
            Info => "info",
            Warning => "warning",
            Error => "error",
            CheckCircle => "check_circle",
            Help => "help",
            Lock => "lock",
            ShoppingCart => "shopping_cart",
            ShoppingBasket => "shopping_basket",
            CreditCard => "credit_card",
            Payment => "payment",
            Notifications => "notifications",
            Visibility => "visibility",
            List => "list",
            Download => "download",
            Upload => "upload",
            OpenInNew => "open_in_new",
            ExitToApp => "exit_to_app",
            ZoomIn => "zoom_in",
            ZoomOut => "zoom_out",
            DateRange => "date_range",
            Event => "event",
            Schedule => "schedule",
            Alarm => "alarm",
            Build => "build",
            Camera => "camera",
            Cloud => "cloud",
            Code => "code",
            Dashboard => "dashboard",
            Flag => "flag",
            Flight => "flight",
            GetApp => "get_app",
            Grade => "grade",
            Headset => "headset",
            History => "history",
            Image => "image",
            InsertChart => "insert_chart",
            Language => "language",
            Lightbulb => "lightbulb",
            LocationOn => "location_on",
            Mic => "mic",
            Movie => "movie",
            MusicNote => "music_note",
            Photo => "photo",
            Place => "place",
            PowerSettingsNew => "power_settings_new",
            Public => "public",
            Receipt => "receipt",
            Restore => "restore",
            Room => "room",
            School => "school",
            Share => "share",
            Sort => "sort",
            TextFields => "text_fields",
            Tune => "tune",
            VerifiedUser => "verified_user",
            Videocam => "videocam",
            VolumeUp => "volume_up",
            VolumeDown => "volume_down",
            VolumeMute => "volume_mute",
            VolumeOff => "volume_off",
            WbSunny => "wb_sunny",
            Work => "work",
            ZoomOutMap => "zoom_out_map",
            Face => "face",
            Feedback => "feedback",
            FileCopy => "file_copy",
            Timer => "timer",
            Toc => "toc",
            Chat => "chat",
            Comment => "comment",
            Contacts => "contacts",
            DragIndicator => "drag_indicator",
            RadioButtonChecked => "radio_button_checked",
            SkipNext => "skip_next",
            SkipPrevious => "skip_previous",
            ThumbsUpDown => "thumbs_up_down",
            Bookmark => "bookmark",
            AttachFile => "attach_file",
            Fullscreen => "fullscreen",
            Print => "print",
            ContentCopy => "content_copy",
            FilterList => "filter_list",
            CloseFullscreen => "close_fullscreen",
            RotateLeft => "rotate_left",
            RotateRight => "rotate_right",
            Rotate90DegreesCcw => "rotate_90_degrees_ccw",
            Rotate90DegreesCw => "rotate_90_degrees_cw",
            Flip => "flip",
            FlipToBack => "flip_to_back",
            FlipToFront => "flip_to_front",
            // 导航
            ArrowDropDown => "arrow_drop_down",
            ArrowDropUp => "arrow_drop_up",
            ArrowCircleDown => "arrow_circle_down",
            ArrowCircleUp => "arrow_circle_up",
            ArrowCircleLeft => "arrow_circle_left",
            ArrowCircleRight => "arrow_circle_right",
            FirstPage => "first_page",
            LastPage => "last_page",
            SubdirectoryArrowLeft => "subdirectory_arrow_left",
            SubdirectoryArrowRight => "subdirectory_arrow_right",
            // 切换/选择
            CheckBox => "check_box",
            CheckBoxOutlineBlank => "check_box_outline_blank",
            RadioButtonUnchecked => "radio_button_unchecked",
            ToggleOff => "toggle_off",
            ToggleOn => "toggle_on",
            StarHalf => "star_half",
            // 文本格式
            FormatBold => "format_bold",
            FormatItalic => "format_italic",
            FormatUnderlined => "format_underlined",
            FormatStrikethrough => "format_strikethrough",
            FormatAlignLeft => "format_align_left",
            FormatAlignCenter => "format_align_center",
            FormatAlignRight => "format_align_right",
            FormatAlignJustify => "format_align_justify",
            FormatListBulleted => "format_list_bulleted",
            FormatListNumbered => "format_list_numbered",
            FormatColorFill => "format_color_fill",
            FormatPaint => "format_paint",
            FormatQuote => "format_quote",
            FormatSize => "format_size",
            // 媒体播放
            Shuffle => "shuffle",
            Repeat => "repeat",
            RepeatOne => "repeat_one",
            Loop => "loop",
            FastForward => "fast_forward",
            FastRewind => "fast_rewind",
            PlaylistAdd => "playlist_add",
            PlaylistPlay => "playlist_play",
            QueueMusic => "queue_music",
            // 剪贴板
            ContentCut => "content_cut",
            ContentPaste => "content_paste",
            // 文件
            FolderOpen => "folder_open",
            FileDownload => "file_download",
            FileUpload => "file_upload",
            CreateNewFolder => "create_new_folder",
            // 动作
            MoreHoriz => "more_horiz",
            Cached => "cached",
            AutoRenew => "autorenew",
            RestartAlt => "restart_alt",
            SwapHoriz => "swap_horiz",
            SwapVert => "swap_vert",
            CompareArrows => "compare_arrows",
            Palette => "palette",
            ColorLens => "color_lens",
            Layers => "layers",
            Apps => "apps",
            Widgets => "widgets",
            // 设备状态
            Wifi => "wifi",
            WifiOff => "wifi_off",
            Bluetooth => "bluetooth",
            BluetoothConnected => "bluetooth_connected",
            BluetoothDisabled => "bluetooth_disabled",
            BatteryFull => "battery_full",
            BatteryChargingFull => "battery_charging_full",
            GpsFixed => "gps_fixed",
            GpsOff => "gps_off",
            Storage => "storage",
            // 通信
            ChatBubble => "chat_bubble",
            ChatBubbleOutline => "chat_bubble_outline",
            Markunread => "markunread",
            AlternateEmail => "alternate_email",
            Call => "call",
            CallEnd => "call_end",
            RingVolume => "ring_volume",
            Dialpad => "dialpad",
            SpeakerPhone => "speaker_phone",
            // 硬件
            Keyboard => "keyboard",
            KeyboardCapslock => "keyboard_capslock",
            KeyboardHide => "keyboard_hide",
            KeyboardReturn => "keyboard_return",
            KeyboardTab => "keyboard_tab",
            Mouse => "mouse",
            Headphones => "headphones",
            HeadsetMic => "headset_mic",
            Laptop => "laptop",
            DesktopWindows => "desktop_windows",
            DesktopMac => "desktop_mac",
            Gamepad => "gamepad",
            Computer => "computer",
            // 社交
            GroupAdd => "group_add",
            People => "people",
            PeopleOutline => "people_outline",
            PersonOutline => "person_outline",
            PersonRemove => "person_remove",
            EmojiEmotions => "emoji_emotions",
            Mood => "mood",
            MoodBad => "mood_bad",
            // 地图/地点
            Map => "map",
            Directions => "directions",
            DirectionsWalk => "directions_walk",
            DirectionsRun => "directions_run",
            Restaurant => "restaurant",
            LocalCafe => "local_cafe",
            LocalDining => "local_dining",
            Store => "store",
            Hotel => "hotel",
            LocalHospital => "local_hospital",
            LocalParking => "local_parking",
            LocalPharmacy => "local_pharmacy",
            LocalOffer => "local_offer",
            LocalGroceryStore => "local_grocery_store",
            LocalMall => "local_mall",
            LocalShipping => "local_shipping",
            LocalTaxi => "local_taxi",
            LocalAtm => "local_atm",
            LocalPhone => "local_phone",
            LocalPizza => "local_pizza",
            LocalPostOffice => "local_post_office",
            LocalPrintShop => "local_print_shop",
            Navigation => "navigation",
            NearMe => "near_me",
            RateReview => "rate_review",
            DepartureBoard => "departure_board",
            // 图像
            AddAPhoto => "add_a_photo",
            AddPhotoAlternate => "add_photo_alternate",
            CameraAlt => "camera_alt",
            Collections => "collections",
            Panorama => "panorama",
            PhotoCamera => "photo_camera",
            PhotoLibrary => "photo_library",
            PhotoAlbum => "photo_album",
            PhotoFilter => "photo_filter",
            Nature => "nature",
            NaturePeople => "nature_people",
            Landscape => "landscape",
            Looks => "looks",
            Portrait => "portrait",
            WbCloudy => "wb_cloudy",
            WbTwilight => "wb_twilight",
        }
    }
}

impl FromStr for IconName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use IconName::*;
        match s {
            "add" => Ok(Add),
            "remove" => Ok(Remove),
            "close" => Ok(Close),
            "check" => Ok(Check),
            "done" => Ok(Done),
            "search" => Ok(Search),
            "settings" => Ok(Settings),
            "delete" => Ok(Delete),
            "delete_forever" => Ok(DeleteForever),
            "menu" => Ok(Menu),
            "more_vert" => Ok(MoreVert),
            "arrow_back" => Ok(ArrowBack),
            "arrow_forward" => Ok(ArrowForward),
            "arrow_up" => Ok(ArrowUp),
            "arrow_down" => Ok(ArrowDown),
            "chevron_right" => Ok(ChevronRight),
            "keyboard_arrow_right" => Ok(KeyboardArrowRight),
            "keyboard_arrow_left" => Ok(KeyboardArrowLeft),
            "keyboard_arrow_up" => Ok(KeyboardArrowUp),
            "keyboard_arrow_down" => Ok(KeyboardArrowDown),
            "expand_more" => Ok(ExpandMore),
            "expand_less" => Ok(ExpandLess),
            "home" => Ok(Home),
            "person" => Ok(Person),
            "account_circle" => Ok(AccountCircle),
            "group" => Ok(Group),
            "person_add" => Ok(PersonAdd),
            "folder" => Ok(Folder),
            "star" => Ok(Star),
            "star_border" => Ok(StarBorder),
            "favorite" => Ok(Favorite),
            "favorite_border" => Ok(FavoriteBorder),
            "thumb_up" => Ok(ThumbUp),
            "thumb_down" => Ok(ThumbDown),
            "save" => Ok(Save),
            "save_alt" => Ok(SaveAlt),
            "edit" => Ok(Edit),
            "create" => Ok(Create),
            "play_arrow" => Ok(PlayArrow),
            "pause" => Ok(Pause),
            "stop" => Ok(Stop),
            "refresh" => Ok(Refresh),
            "sync" => Ok(Sync),
            "undo" => Ok(Undo),
            "redo" => Ok(Redo),
            "mail" => Ok(Mail),
            "send" => Ok(Send),
            "phone" => Ok(Phone),
            "info" => Ok(Info),
            "warning" => Ok(Warning),
            "error" => Ok(Error),
            "check_circle" => Ok(CheckCircle),
            "help" => Ok(Help),
            "lock" => Ok(Lock),
            "shopping_cart" => Ok(ShoppingCart),
            "shopping_basket" => Ok(ShoppingBasket),
            "credit_card" => Ok(CreditCard),
            "payment" => Ok(Payment),
            "notifications" => Ok(Notifications),
            "visibility" => Ok(Visibility),
            "list" => Ok(List),
            "download" => Ok(Download),
            "upload" => Ok(Upload),
            "open_in_new" => Ok(OpenInNew),
            "exit_to_app" => Ok(ExitToApp),
            "zoom_in" => Ok(ZoomIn),
            "zoom_out" => Ok(ZoomOut),
            "date_range" => Ok(DateRange),
            "event" => Ok(Event),
            "schedule" => Ok(Schedule),
            "alarm" => Ok(Alarm),
            "build" => Ok(Build),
            "camera" => Ok(Camera),
            "cloud" => Ok(Cloud),
            "code" => Ok(Code),
            "dashboard" => Ok(Dashboard),
            "flag" => Ok(Flag),
            "flight" => Ok(Flight),
            "get_app" => Ok(GetApp),
            "grade" => Ok(Grade),
            "headset" => Ok(Headset),
            "history" => Ok(History),
            "image" => Ok(Image),
            "insert_chart" => Ok(InsertChart),
            "language" => Ok(Language),
            "lightbulb" => Ok(Lightbulb),
            "location_on" => Ok(LocationOn),
            "mic" => Ok(Mic),
            "movie" => Ok(Movie),
            "music_note" => Ok(MusicNote),
            "photo" => Ok(Photo),
            "place" => Ok(Place),
            "power_settings_new" => Ok(PowerSettingsNew),
            "public" => Ok(Public),
            "receipt" => Ok(Receipt),
            "restore" => Ok(Restore),
            "room" => Ok(Room),
            "school" => Ok(School),
            "share" => Ok(Share),
            "sort" => Ok(Sort),
            "text_fields" => Ok(TextFields),
            "tune" => Ok(Tune),
            "verified_user" => Ok(VerifiedUser),
            "videocam" => Ok(Videocam),
            "volume_up" => Ok(VolumeUp),
            "volume_down" => Ok(VolumeDown),
            "volume_mute" => Ok(VolumeMute),
            "volume_off" => Ok(VolumeOff),
            "wb_sunny" => Ok(WbSunny),
            "work" => Ok(Work),
            "zoom_out_map" => Ok(ZoomOutMap),
            "face" => Ok(Face),
            "feedback" => Ok(Feedback),
            "file_copy" => Ok(FileCopy),
            "timer" => Ok(Timer),
            "toc" => Ok(Toc),
            "chat" => Ok(Chat),
            "comment" => Ok(Comment),
            "contacts" => Ok(Contacts),
            "drag_indicator" => Ok(DragIndicator),
            "radio_button_checked" => Ok(RadioButtonChecked),
            "skip_next" => Ok(SkipNext),
            "skip_previous" => Ok(SkipPrevious),
            "thumbs_up_down" => Ok(ThumbsUpDown),
            "bookmark" => Ok(Bookmark),
            "attach_file" => Ok(AttachFile),
            "fullscreen" => Ok(Fullscreen),
            "print" => Ok(Print),
            "content_copy" => Ok(ContentCopy),
            "filter_list" => Ok(FilterList),
            "close_fullscreen" => Ok(CloseFullscreen),
            "rotate_left" => Ok(RotateLeft),
            "rotate_right" => Ok(RotateRight),
            "rotate_90_degrees_ccw" => Ok(Rotate90DegreesCcw),
            "rotate_90_degrees_cw" => Ok(Rotate90DegreesCw),
            "flip" => Ok(Flip),
            "flip_to_back" => Ok(FlipToBack),
            "flip_to_front" => Ok(FlipToFront),
            // 导航
            "arrow_drop_down" => Ok(ArrowDropDown),
            "arrow_drop_up" => Ok(ArrowDropUp),
            "arrow_circle_down" => Ok(ArrowCircleDown),
            "arrow_circle_up" => Ok(ArrowCircleUp),
            "arrow_circle_left" => Ok(ArrowCircleLeft),
            "arrow_circle_right" => Ok(ArrowCircleRight),
            "first_page" => Ok(FirstPage),
            "last_page" => Ok(LastPage),
            "subdirectory_arrow_left" => Ok(SubdirectoryArrowLeft),
            "subdirectory_arrow_right" => Ok(SubdirectoryArrowRight),
            // 切换/选择
            "check_box" => Ok(CheckBox),
            "check_box_outline_blank" => Ok(CheckBoxOutlineBlank),
            "radio_button_unchecked" => Ok(RadioButtonUnchecked),
            "toggle_off" => Ok(ToggleOff),
            "toggle_on" => Ok(ToggleOn),
            "star_half" => Ok(StarHalf),
            // 文本格式
            "format_bold" => Ok(FormatBold),
            "format_italic" => Ok(FormatItalic),
            "format_underlined" => Ok(FormatUnderlined),
            "format_strikethrough" => Ok(FormatStrikethrough),
            "format_align_left" => Ok(FormatAlignLeft),
            "format_align_center" => Ok(FormatAlignCenter),
            "format_align_right" => Ok(FormatAlignRight),
            "format_align_justify" => Ok(FormatAlignJustify),
            "format_list_bulleted" => Ok(FormatListBulleted),
            "format_list_numbered" => Ok(FormatListNumbered),
            "format_color_fill" => Ok(FormatColorFill),
            "format_paint" => Ok(FormatPaint),
            "format_quote" => Ok(FormatQuote),
            "format_size" => Ok(FormatSize),
            // 媒体播放
            "shuffle" => Ok(Shuffle),
            "repeat" => Ok(Repeat),
            "repeat_one" => Ok(RepeatOne),
            "loop" => Ok(Loop),
            "fast_forward" => Ok(FastForward),
            "fast_rewind" => Ok(FastRewind),
            "playlist_add" => Ok(PlaylistAdd),
            "playlist_play" => Ok(PlaylistPlay),
            "queue_music" => Ok(QueueMusic),
            // 剪贴板
            "content_cut" => Ok(ContentCut),
            "content_paste" => Ok(ContentPaste),
            // 文件
            "folder_open" => Ok(FolderOpen),
            "file_download" => Ok(FileDownload),
            "file_upload" => Ok(FileUpload),
            "create_new_folder" => Ok(CreateNewFolder),
            // 动作
            "more_horiz" => Ok(MoreHoriz),
            "cached" => Ok(Cached),
            "autorenew" => Ok(AutoRenew),
            "restart_alt" => Ok(RestartAlt),
            "swap_horiz" => Ok(SwapHoriz),
            "swap_vert" => Ok(SwapVert),
            "compare_arrows" => Ok(CompareArrows),
            "palette" => Ok(Palette),
            "color_lens" => Ok(ColorLens),
            "layers" => Ok(Layers),
            "apps" => Ok(Apps),
            "widgets" => Ok(Widgets),
            // 设备状态
            "wifi" => Ok(Wifi),
            "wifi_off" => Ok(WifiOff),
            "bluetooth" => Ok(Bluetooth),
            "bluetooth_connected" => Ok(BluetoothConnected),
            "bluetooth_disabled" => Ok(BluetoothDisabled),
            "battery_full" => Ok(BatteryFull),
            "battery_charging_full" => Ok(BatteryChargingFull),
            "gps_fixed" => Ok(GpsFixed),
            "gps_off" => Ok(GpsOff),
            "storage" => Ok(Storage),
            // 通信
            "chat_bubble" => Ok(ChatBubble),
            "chat_bubble_outline" => Ok(ChatBubbleOutline),
            "markunread" => Ok(Markunread),
            "alternate_email" => Ok(AlternateEmail),
            "call" => Ok(Call),
            "call_end" => Ok(CallEnd),
            "ring_volume" => Ok(RingVolume),
            "dialpad" => Ok(Dialpad),
            "speaker_phone" => Ok(SpeakerPhone),
            // 硬件
            "keyboard" => Ok(Keyboard),
            "keyboard_capslock" => Ok(KeyboardCapslock),
            "keyboard_hide" => Ok(KeyboardHide),
            "keyboard_return" => Ok(KeyboardReturn),
            "keyboard_tab" => Ok(KeyboardTab),
            "mouse" => Ok(Mouse),
            "headphones" => Ok(Headphones),
            "headset_mic" => Ok(HeadsetMic),
            "laptop" => Ok(Laptop),
            "desktop_windows" => Ok(DesktopWindows),
            "desktop_mac" => Ok(DesktopMac),
            "gamepad" => Ok(Gamepad),
            "computer" => Ok(Computer),
            // 社交
            "group_add" => Ok(GroupAdd),
            "people" => Ok(People),
            "people_outline" => Ok(PeopleOutline),
            "person_outline" => Ok(PersonOutline),
            "person_remove" => Ok(PersonRemove),
            "emoji_emotions" => Ok(EmojiEmotions),
            "mood" => Ok(Mood),
            "mood_bad" => Ok(MoodBad),
            // 地图/地点
            "map" => Ok(Map),
            "directions" => Ok(Directions),
            "directions_walk" => Ok(DirectionsWalk),
            "directions_run" => Ok(DirectionsRun),
            "restaurant" => Ok(Restaurant),
            "local_cafe" => Ok(LocalCafe),
            "local_dining" => Ok(LocalDining),
            "store" => Ok(Store),
            "hotel" => Ok(Hotel),
            "local_hospital" => Ok(LocalHospital),
            "local_parking" => Ok(LocalParking),
            "local_pharmacy" => Ok(LocalPharmacy),
            "local_offer" => Ok(LocalOffer),
            "local_grocery_store" => Ok(LocalGroceryStore),
            "local_mall" => Ok(LocalMall),
            "local_shipping" => Ok(LocalShipping),
            "local_taxi" => Ok(LocalTaxi),
            "local_atm" => Ok(LocalAtm),
            "local_phone" => Ok(LocalPhone),
            "local_pizza" => Ok(LocalPizza),
            "local_post_office" => Ok(LocalPostOffice),
            "local_print_shop" => Ok(LocalPrintShop),
            "navigation" => Ok(Navigation),
            "near_me" => Ok(NearMe),
            "rate_review" => Ok(RateReview),
            "departure_board" => Ok(DepartureBoard),
            // 图像
            "add_a_photo" => Ok(AddAPhoto),
            "add_photo_alternate" => Ok(AddPhotoAlternate),
            "camera_alt" => Ok(CameraAlt),
            "collections" => Ok(Collections),
            "panorama" => Ok(Panorama),
            "photo_camera" => Ok(PhotoCamera),
            "photo_library" => Ok(PhotoLibrary),
            "photo_album" => Ok(PhotoAlbum),
            "photo_filter" => Ok(PhotoFilter),
            "nature" => Ok(Nature),
            "nature_people" => Ok(NaturePeople),
            "landscape" => Ok(Landscape),
            "looks" => Ok(Looks),
            "portrait" => Ok(Portrait),
            "wb_cloudy" => Ok(WbCloudy),
            "wb_twilight" => Ok(WbTwilight),
            _ => Err(format!("unknown icon name: '{s}'")),
        }
    }
}

/// 图标组件：用图标字体渲染单个字形。
///
/// 仅渲染字形本身；需要按钮外壳、点击与 hover/pressed 反馈时用 [`IconButton`]。
#[derive(Clone)]
pub struct Icon {
    name: IconName,
    size: f64,
    color: Color,
    hover_color: Option<Color>,
    pressed_color: Option<Color>,
    font_family: String,
    listeners: Vec<Listener>,
}

impl Icon {
    pub fn new(name: IconName, size: f64) -> Self {
        Self {
            name,
            size,
            color: theme::current().text.regular_default,
            hover_color: None,
            pressed_color: None,
            font_family: IconName::FONT_FAMILY.to_string(),
            listeners: Vec::new(),
        }
    }

    /// 字号（像素），即图标尺寸。
    pub fn size(mut self, v: f64) -> Self {
        self.size = v;
        self
    }

    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    pub fn hover_color(mut self, c: Color) -> Self {
        self.hover_color = Some(c);
        self
    }

    pub fn pressed_color(mut self, c: Color) -> Self {
        self.pressed_color = Some(c);
        self
    }

    /// 自定义图标字体的 family 名（默认 "Material Icons"）。
    pub fn font_family(mut self, f: impl Into<String>) -> Self {
        self.font_family = f.into();
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }

    pub fn on_click_with_ctx<F: Fn(&mut crate::event::EventContext) + 'static>(
        mut self,
        f: F,
    ) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }
}

impl Widget for Icon {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        ensure_icon_font_registered();
        let mut style = TextStyle::new()
            .font_size(self.size)
            .color(self.color)
            .font_family(self.font_family.clone())
            .wrap(false);
        if let Some(h) = self.hover_color {
            style = style.hover_color(h);
        }
        if let Some(p) = self.pressed_color {
            style = style.pressed_color(p);
        }

        ViewNode::Text {
            content: self.name.char().to_string(),
            style,
            layout: FlexStyle::default().align_self(FlexAlign::Center),
            key: None,
            listeners: self.listeners.clone(),
        }
    }
}

/// IconButton 变体（视觉风格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonVariant {
    /// 工具栏默认：透明背景，hover 浅灰底 + 品牌色图标。
    #[default]
    Plain,
    /// 描边按钮：白底 + 边框。
    Outline,
    /// 品牌色实底 + 白色图标。
    Primary,
}

/// 工具栏风格的图标按钮：方形外壳 + 居中图标，hover/pressed 背景与图标变色。
///
/// 这是通用 [`Button`] 的便捷封装：内容固定为 [`Icon`]，默认正方形边长、hover 变色等
/// 语义与通用按钮保持一致。如需更复杂的内部内容（文本/自定义 widget），直接用 [`Button`]。
pub struct IconButton {
    button: Button,
}

impl IconButton {
    pub fn new(icon: IconName) -> Self {
        Self {
            button: Button::icon(icon, 18.0)
                .fixed_size(28.0)
                .variant(ButtonVariant::Plain)
                .flex_shrink(1.0),
        }
    }

    /// 设置按钮是否禁用。禁用后不响应点击与悬停，图标灰显。
    pub fn disabled(mut self, v: bool) -> Self {
        self.button = self.button.disabled(v);
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.button = self.button.on_click(f);
        self
    }

    pub fn on_click_with_ctx<F: Fn(&mut crate::event::EventContext) + 'static>(
        mut self,
        f: F,
    ) -> Self {
        self.button = self.button.on_click_with_ctx(f);
        self
    }

    /// 鼠标进入回调（指针移入按钮区域时触发）。
    pub fn on_mouse_enter<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.button = self.button.on_mouse_enter(f);
        self
    }

    /// 鼠标离开回调（指针移出按钮区域时触发）。
    pub fn on_mouse_leave<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.button = self.button.on_mouse_leave(f);
        self
    }

    /// 按钮边长（默认 28）。
    pub fn size(mut self, v: f32) -> Self {
        self.button = self.button.fixed_size(v);
        self
    }

    /// 图标字号（默认 18）。
    pub fn icon_size(mut self, v: f64) -> Self {
        self.button = self.button.icon_size(v);
        self
    }

    /// 圆角半径（默认主题 small）。
    pub fn radius(mut self, v: f32) -> Self {
        self.button = self.button.radius(v);
        self
    }

    pub fn variant(mut self, v: IconButtonVariant) -> Self {
        self.button = self.button.variant(v.to_button_variant());
        self
    }

    pub fn icon_color(mut self, c: Color) -> Self {
        self.button = self.button.icon_color(c);
        self
    }

    pub fn icon_hover_color(mut self, c: Color) -> Self {
        self.button = self.button.icon_hover_color(c);
        self
    }

    pub fn icon_pressed_color(mut self, c: Color) -> Self {
        self.button = self.button.icon_pressed_color(c);
        self
    }

    /// flex 收缩因子（默认 1.0）。
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.button = self.button.flex_shrink(v);
        self
    }

    /// 设置工具提示文本（鼠标悬停时显示）。
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.button = self.button.tooltip(text);
        self
    }
}

impl IconButtonVariant {
    /// 映射为通用 [`ButtonVariant`]，视觉语义一一对应。
    fn to_button_variant(self) -> ButtonVariant {
        match self {
            IconButtonVariant::Plain => ButtonVariant::Plain,
            IconButtonVariant::Outline => ButtonVariant::Secondary,
            IconButtonVariant::Primary => ButtonVariant::Primary,
        }
    }
}

impl Widget for IconButton {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        self.button.build(ctx)
    }
}
