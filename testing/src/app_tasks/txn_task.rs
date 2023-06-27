use hotshot::tasks::DATaskState;
use hotshot_task::task::{TaskErr, TS};
use snafu::Snafu;

/// Data Availability task error
#[derive(Snafu, Debug)]
pub struct TimeoutTaskErr {}
impl TaskErr for TimeoutTaskErr {}

/// Data availability task state
#[derive(Debug)]
pub struct TimeoutTask {}
impl TS for TimeoutTask {}

// /// Data Availability task types
// pub type DATaskTypes =
//     HSTWithEvent<TimeoutTaskErr, GlobalEvent, ChannelStream<GlobalEvent>, DATaskState>;
