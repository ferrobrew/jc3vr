# Environment controls

The debug UI's Tools > Environment feature (`crates/jc3vr-ui/src/ui/environment.rs`) changes the time of day and the weather, for testing rendering under different lighting. It reads and writes the game's singletons directly and re-reads each value every frame, so drift shows. [engine/environment.md](../engine/environment.md) describes the game's side.

Time of day is an hour slider that calls `WorldTime::SetTimeOfDay`, which keeps the game's clamp and per-hour event, and a slider for the day cycle's speed. The weather has a row of buttons, Sunny, Rain, Snow, Restore, and Instant, that send the weather controller's named events through `jc3gi::event::send_event_msg`: this is the way to pin a weather state. Below them are a severity slider that calls `Weather::SetSeverity` and direct rain intensity and snow ratio sliders, which the weather controller pulls back toward its own targets each frame unless an event holds the state.
