//! One-byte codes from protocol.md §5. Each code's byte value is written once, in its `From<_> for u8` match;
//! decoding searches `ALL`. `MessageType` and `StreamId` decode unknown bytes to `Unknown(byte)` because the drone
//! must still answer them; the other enums reject unknown bytes with `UnknownCode`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownCode(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageType {
    Hello,
    HelloReply,
    Ping,
    Pong,
    LinkTest,
    LinkTestData,
    LinkTestResult,
    Sticks,
    Arm,
    Disarm,
    TakeOff,
    Land,
    GoHome,
    Flip,
    SteadyState,
    ExitSteadyState,
    Reply,
    Event,
    SetSettings,
    GetSettings,
    Settings,
    RequestOnce,
    Subscribe,
    StreamData,
    ListLogs,
    LogList,
    ReadLog,
    LogChunk,
    Unknown(u8), // only produced by decoding, never holds a known code
}

impl MessageType {
    pub const ALL: [MessageType; 28] = [
        Self::Hello,
        Self::HelloReply,
        Self::Ping,
        Self::Pong,
        Self::LinkTest,
        Self::LinkTestData,
        Self::LinkTestResult,
        Self::Sticks,
        Self::Arm,
        Self::Disarm,
        Self::TakeOff,
        Self::Land,
        Self::GoHome,
        Self::Flip,
        Self::SteadyState,
        Self::ExitSteadyState,
        Self::Reply,
        Self::Event,
        Self::SetSettings,
        Self::GetSettings,
        Self::Settings,
        Self::RequestOnce,
        Self::Subscribe,
        Self::StreamData,
        Self::ListLogs,
        Self::LogList,
        Self::ReadLog,
        Self::LogChunk,
    ];
}

impl From<MessageType> for u8 {
    fn from(message_type: MessageType) -> u8 {
        return match message_type {
            MessageType::Hello => 0x01,
            MessageType::HelloReply => 0x02,
            MessageType::Ping => 0x03,
            MessageType::Pong => 0x04,
            MessageType::LinkTest => 0x05,
            MessageType::LinkTestData => 0x06,
            MessageType::LinkTestResult => 0x07,
            MessageType::Sticks => 0x10,
            MessageType::Arm => 0x20,
            MessageType::Disarm => 0x21,
            MessageType::TakeOff => 0x22,
            MessageType::Land => 0x23,
            MessageType::GoHome => 0x24,
            MessageType::Flip => 0x25,
            MessageType::SteadyState => 0x26,
            MessageType::ExitSteadyState => 0x27,
            MessageType::Reply => 0x40,
            MessageType::Event => 0x41,
            MessageType::SetSettings => 0x50,
            MessageType::GetSettings => 0x51,
            MessageType::Settings => 0x52,
            MessageType::RequestOnce => 0x60,
            MessageType::Subscribe => 0x61,
            MessageType::StreamData => 0x62,
            MessageType::ListLogs => 0x70,
            MessageType::LogList => 0x71,
            MessageType::ReadLog => 0x72,
            MessageType::LogChunk => 0x73,
            MessageType::Unknown(byte) => byte,
        };
    }
}

impl From<u8> for MessageType {
    fn from(byte: u8) -> Self {
        return Self::ALL
            .into_iter()
            .find(|&t| return u8::from(t) == byte)
            .unwrap_or(Self::Unknown(byte));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloResult {
    Ok,
    VersionMismatch,
    OptionNotSupported,
}

impl HelloResult {
    pub const ALL: [HelloResult; 3] = [Self::Ok, Self::VersionMismatch, Self::OptionNotSupported];
}

impl From<HelloResult> for u8 {
    fn from(result: HelloResult) -> u8 {
        return match result {
            HelloResult::Ok => 0,
            HelloResult::VersionMismatch => 1,
            HelloResult::OptionNotSupported => 2,
        };
    }
}

impl TryFrom<u8> for HelloResult {
    type Error = UnknownCode;

    fn try_from(byte: u8) -> Result<Self, UnknownCode> {
        return Self::ALL
            .into_iter()
            .find(|&r| return u8::from(r) == byte)
            .ok_or(UnknownCode(byte));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkTestDirection {
    DroneSends,
    ClientSends,
}

impl LinkTestDirection {
    pub const ALL: [LinkTestDirection; 2] = [Self::DroneSends, Self::ClientSends];
}

impl From<LinkTestDirection> for u8 {
    fn from(direction: LinkTestDirection) -> u8 {
        return match direction {
            LinkTestDirection::DroneSends => 0,
            LinkTestDirection::ClientSends => 1,
        };
    }
}

impl TryFrom<u8> for LinkTestDirection {
    type Error = UnknownCode;

    fn try_from(byte: u8) -> Result<Self, UnknownCode> {
        return Self::ALL
            .into_iter()
            .find(|&d| return u8::from(d) == byte)
            .ok_or(UnknownCode(byte));
    }
}

/// Status of a Reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Refused,
    UnknownType,
    Malformed,
    NotSupported,
    Busy,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Self::Ok,
        Self::Refused,
        Self::UnknownType,
        Self::Malformed,
        Self::NotSupported,
        Self::Busy,
    ];
}

impl From<Status> for u8 {
    fn from(status: Status) -> u8 {
        return match status {
            Status::Ok => 0,
            Status::Refused => 1,
            Status::UnknownType => 2,
            Status::Malformed => 3,
            Status::NotSupported => 4,
            Status::Busy => 5,
        };
    }
}

impl TryFrom<u8> for Status {
    type Error = UnknownCode;

    fn try_from(byte: u8) -> Result<Self, UnknownCode> {
        return Self::ALL
            .into_iter()
            .find(|&s| return u8::from(s) == byte)
            .ok_or(UnknownCode(byte));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl EventLevel {
    pub const ALL: [EventLevel; 4] = [Self::Debug, Self::Info, Self::Warning, Self::Error];
}

impl From<EventLevel> for u8 {
    fn from(level: EventLevel) -> u8 {
        return match level {
            EventLevel::Debug => 0,
            EventLevel::Info => 1,
            EventLevel::Warning => 2,
            EventLevel::Error => 3,
        };
    }
}

impl TryFrom<u8> for EventLevel {
    type Error = UnknownCode;

    fn try_from(byte: u8) -> Result<Self, UnknownCode> {
        return Self::ALL
            .into_iter()
            .find(|&l| return u8::from(l) == byte)
            .ok_or(UnknownCode(byte));
    }
}

/// Units per stream are in protocol.md §5.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamId {
    Status,
    Battery,
    Attitude,
    Height,
    RawImu,
    Gps, // reserved until GPS is fitted
    LinkQuality,
    Unknown(u8), // only produced by decoding, never holds a known code
}

impl StreamId {
    pub const ALL: [StreamId; 7] = [
        Self::Status,
        Self::Battery,
        Self::Attitude,
        Self::Height,
        Self::RawImu,
        Self::Gps,
        Self::LinkQuality,
    ];
}

impl From<StreamId> for u8 {
    fn from(stream: StreamId) -> u8 {
        return match stream {
            StreamId::Status => 1,
            StreamId::Battery => 2,
            StreamId::Attitude => 3,
            StreamId::Height => 4,
            StreamId::RawImu => 5,
            StreamId::Gps => 6,
            StreamId::LinkQuality => 7,
            StreamId::Unknown(byte) => byte,
        };
    }
}

impl From<u8> for StreamId {
    fn from(byte: u8) -> Self {
        return Self::ALL
            .into_iter()
            .find(|&s| return u8::from(s) == byte)
            .unwrap_or(Self::Unknown(byte));
    }
}
