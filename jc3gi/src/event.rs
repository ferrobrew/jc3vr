#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A sent event as its receivers see it (`NEvent::CEventImpl`). Opaque.
pub struct EventImpl {}
impl EventImpl {}
impl std::convert::AsRef<EventImpl> for EventImpl {
    fn as_ref(&self) -> &EventImpl {
        self
    }
}
impl std::convert::AsMut<EventImpl> for EventImpl {
    fn as_mut(&mut self) -> &mut EventImpl {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A subscription to a named event (`NEvent::CReceiveEvent`): the handle of the subscription's
/// entry in the event system's registry, or zero when unsubscribed.
pub struct ReceiveEvent {
    pub m_Handle: u32,
}
fn _ReceiveEvent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ReceiveEvent>([0u8; 0x4]);
    }
    unreachable!()
}
impl ReceiveEvent {}
impl std::convert::AsRef<ReceiveEvent> for ReceiveEvent {
    fn as_ref(&self) -> &ReceiveEvent {
        self
    }
}
impl std::convert::AsMut<ReceiveEvent> for ReceiveEvent {
    fn as_mut(&mut self) -> &mut ReceiveEvent {
        self
    }
}
pub const send_event_msg_impl_ADDRESS: usize = 0x1400719D0;
/// `NEvent::CSendEvent::SendMsg(const char*, void*)`: fires a named engine event to every
/// subscribed receiver, with an optional message payload (null for fire-and-forget). This is the
/// engine's own idiom for global events (`CDialogue::Play` fires `on_play_dialogue` through it).
/// Systems subscribe named events in their `Init`; `CWeatherController` subscribes the weather
/// events (`weather_sunny`, `weather_rain`, `weather_snow`, `weather_restore`, `weather_instant`,
/// `cloud_base`, and `cloud_height`).
unsafe fn send_event_msg_impl(name: *const u8, data: *mut u8) {
    unsafe {
        let f: unsafe extern "system" fn(name: *const u8, data: *mut u8) = ::std::mem::transmute(
            send_event_msg_impl_ADDRESS,
        );
        f(name, data)
    }
}
pub const EventMatches_ADDRESS: usize = 0x14006DA40;
/// `NEvent::operator==(const CEventImpl*, const CReceiveEvent&)`: whether `event` is the event
/// `receiver` subscribes to. An event handler that subscribes several events tells them apart
/// with it. An unsubscribed receiver matches nothing.
pub unsafe fn EventMatches(
    event: *const crate::event::EventImpl,
    receiver: *const crate::event::ReceiveEvent,
) -> bool {
    unsafe {
        let f: unsafe extern "system" fn(
            event: *const crate::event::EventImpl,
            receiver: *const crate::event::ReceiveEvent,
        ) -> bool = ::std::mem::transmute(EventMatches_ADDRESS);
        f(event, receiver)
    }
}
/// Fire a named engine event with no payload.
pub fn send_event_msg(name: &std::ffi::CStr) {
    unsafe { send_event_msg_impl(name.as_ptr() as *const u8, std::ptr::null_mut()) }
}
