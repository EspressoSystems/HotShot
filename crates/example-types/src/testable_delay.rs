use async_trait::async_trait;

#[derive(Clone, Copy, Debug)]
pub enum DelayOptions {
    None,
    Random,
    Fixed,
}

#[async_trait]
pub trait TestableDelay {
    async fn handle_delay_option(delay_option: DelayOptions);
}
