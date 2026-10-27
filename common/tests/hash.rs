//! Every public value type can be a `HashMap`/`HashSet` key, e.g. for the client to track what it sent.
use drone_common::domain::*;
use drone_common::wire::event::*;
use drone_common::wire::message::*;
use drone_common::wire::packet::{Packet, PacketError};
use drone_common::wire::reply::*;
use std::hash::Hash;

/// Only compiles if `T` is a valid key; the name makes failures readable.
fn key_type<T: Hash + Eq>() -> &'static str {
    return std::any::type_name::<T>();
}

#[test]
fn public_types_are_hashable() {
    let names = [
        key_type::<Message>(),
        key_type::<Hello>(),
        key_type::<HelloReply>(),
        key_type::<Options>(),
        key_type::<Ping>(),
        key_type::<Pong>(),
        key_type::<Arm>(),
        key_type::<Flip>(),
        key_type::<Command>(),
        key_type::<UnknownType>(),
        key_type::<RequestId>(),
        key_type::<Reply>(),
        key_type::<Event>(),
        key_type::<EventId>(),
        key_type::<EventKind>(),
        key_type::<Ack>(),
        key_type::<TakeOff>(),
        key_type::<Armed>(),
        key_type::<BatteryLow>(),
        key_type::<Text>(),
        key_type::<FailsafeCause>(),
        key_type::<EventText>(),
        key_type::<MessageError>(),
        key_type::<PacketError>(),
        key_type::<Packet<'static>>(),
        key_type::<Sticks>(),
        key_type::<ArmState>(),
        key_type::<ArmSwitch>(),
        key_type::<LinkHealth>(),
        key_type::<Uptime>(),
        key_type::<Heard>(),
        key_type::<RcChannels>(),
        key_type::<HelloResult>(),
        key_type::<EventLevel>(),
        key_type::<Answer>(),
        key_type::<RequestType>(),
        key_type::<Reason>(),
        key_type::<FlipRefusal>(),
        key_type::<Outcome<ArmRefusal>>(),
        key_type::<ArmRefusal>(),
        key_type::<UnknownOptions>(),
    ];
    assert!(names.iter().all(|name| return !name.is_empty()));
}

#[test]
fn equal_event_texts_hash_equal() {
    use std::collections::HashSet;
    let texts: HashSet<EventText> = ["a", "a", "b"].map(|t| return EventText::new(t).unwrap()).into();
    assert_eq!(texts.len(), 2);
}
