use std::marker::PhantomData;

// use async_std::task::JoinHandle;
// use async_trait::async_trait;
// use flume::Sender;

// #[async_trait]
// trait TaskMessaging {
// type Message;
// async fn control(&mut self, message: Self::Message);
// async fn get_receiver(&mut self) -> ReceiverTask
// }

/// an easy way to manage a task via message
pub enum TaskMessage<M> {
    Shutdown,
    Timeout,
    Message(M),
}

// pub struct TaskChannel<M> {
//     sender: u32,
//     receiver: u32
// }

// pub trait HotShotTask<M> {
//
//
//
// }
//
// impl<M> TaskMessage<M> {
//     pub fn shutdown() {
//     }
//
// }

// an easy way to manage tasks
// struct Runner<T: ReceiverTask> {
//     task: JoinHandle<()>,
//     sender: Sender<ReceiverTaskMessage<T::Message>>,
//     pd: PhantomData<T>,
// }
//
// /// implementation of a runner
// impl<T: ReceiverTask> Runner<T> {
//     /// shut down the task
//     pub fn shutdown(&self) {
//         self.sender.send(ReceiverTaskMessage::Shutdown);
//     }
//     /// time out task
//     pub fn timeout(&self) {
//         self.sender.send(ReceiverTaskMessage::Timeout);
//     }
//     /// send a message to the task
//     pub fn send(&self, message: T::Message) {
//         self.sender.send(ReceiverTaskMessage::Message(message));
//     }
// }
