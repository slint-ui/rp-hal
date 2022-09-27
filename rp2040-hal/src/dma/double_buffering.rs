use core::sync::atomic::{compiler_fence, Ordering};

use super::{
    single_channel::ChannelConfig, single_channel::SingleChannel, EndlessReadTarget,
    EndlessWriteTarget, Pace, ReadTarget, WriteTarget,
};

/// Configuration for double-buffered DMA transfer
pub struct DoubleBufferingConfig<
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget,
    TO: WriteTarget,
> {
    ch: (CH1, CH2),
    from: FROM,
    to: TO,
    pace: Pace,
}

impl<CH1, CH2, FROM, TO, WORD> DoubleBufferingConfig<CH1, CH2, FROM, TO>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD>,
    TO: WriteTarget<TransmittedWord = WORD>,
{
    /// Create a new configuration for double-buffered DMA transfer
    pub fn new(ch: (CH1, CH2), from: FROM, to: TO) -> DoubleBufferingConfig<CH1, CH2, FROM, TO> {
        DoubleBufferingConfig {
            ch,
            from,
            to,
            pace: Pace::PreferSource,
        }
    }

    /// Sets the (preferred) pace for the DMA transfers.
    ///
    /// Usually, the code will automatically configure the correct pace, but
    /// peripheral-to-peripheral transfers require the user to manually select whether the source
    /// or the sink shall be queried for the pace signal.
    pub fn pace(&mut self, pace: Pace) {
        self.pace = pace;
    }

    /// Start the DMA transfer
    pub fn start(mut self) -> DoubleBuffering<CH1, CH2, FROM, TO, ()> {
        // TODO: Do we want to call any callbacks to configure source/sink?

        // Make sure that memory contents reflect what the user intended.
        // TODO: How much of the following is necessary?
        cortex_m::asm::dsb();
        compiler_fence(Ordering::SeqCst);

        // Configure the DMA channel and start it.
        self.ch
            .0
            .config(&self.from, &mut self.to, self.pace, None, true);

        DoubleBuffering {
            ch: self.ch,
            from: self.from,
            to: self.to,
            pace: self.pace,
            state: (),
            second_ch: false,
        }
    }
}

/// State for a double-buffered read
pub struct ReadNext<BUF: ReadTarget>(BUF);
/// State for a double-buffered write
pub struct WriteNext<BUF: WriteTarget>(BUF);

/// Instance of a double-buffered DMA transfer
pub struct DoubleBuffering<CH1, CH2, FROM, TO, STATE>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget,
    TO: WriteTarget,
{
    ch: (CH1, CH2),
    from: FROM,
    to: TO,
    pace: Pace,
    state: STATE,
    second_ch: bool,
}

impl<CH1, CH2, FROM, TO, WORD, STATE> DoubleBuffering<CH1, CH2, FROM, TO, STATE>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD>,
    TO: WriteTarget<TransmittedWord = WORD>,
{
    /// Check if the transfer is completed
    pub fn is_done(&self) -> bool {
        if self.second_ch {
            !self.ch.1.ch().ch_ctrl_trig.read().busy().bit_is_set()
        } else {
            !self.ch.0.ch().ch_ctrl_trig.read().busy().bit_is_set()
        }
    }
}

impl<CH1, CH2, FROM, TO, WORD> DoubleBuffering<CH1, CH2, FROM, TO, ()>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD>,
    TO: WriteTarget<TransmittedWord = WORD> + EndlessWriteTarget,
{
    /// Block until transfer completed
    pub fn wait(self) -> (CH1, CH2, FROM, TO) {
        while !self.is_done() {}

        // Make sure that memory contents reflect what the user intended.
        cortex_m::asm::dsb();
        compiler_fence(Ordering::SeqCst);

        // TODO: Use a tuple type?
        (self.ch.0, self.ch.1, self.from, self.to)
    }
}

impl<CH1, CH2, FROM, TO, WORD> DoubleBuffering<CH1, CH2, FROM, TO, ()>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD>,
    TO: WriteTarget<TransmittedWord = WORD> + EndlessWriteTarget,
{
    /// Perform the next read of a double-buffered sequence
    pub fn read_next<BUF: ReadTarget<ReceivedWord = WORD>>(
        mut self,
        buf: BUF,
    ) -> DoubleBuffering<CH1, CH2, FROM, TO, ReadNext<BUF>> {
        // Make sure that memory contents reflect what the user intended.
        // TODO: How much of the following is necessary?
        cortex_m::asm::dsb();
        compiler_fence(Ordering::SeqCst);

        // Configure the _other_ DMA channel, but do not start it yet.
        if self.second_ch {
            self.ch.0.config(&buf, &mut self.to, self.pace, None, false);
        } else {
            self.ch.1.config(&buf, &mut self.to, self.pace, None, false);
        }

        // Chain the first channel to the second.
        if self.second_ch {
            self.ch.1.set_chain_to_enabled(&mut self.ch.0);
        } else {
            self.ch.0.set_chain_to_enabled(&mut self.ch.1);
        }

        DoubleBuffering {
            ch: self.ch,
            from: self.from,
            to: self.to,
            pace: self.pace,
            state: ReadNext(buf),
            second_ch: self.second_ch,
        }
    }
}

impl<CH1, CH2, FROM, TO, WORD> DoubleBuffering<CH1, CH2, FROM, TO, ()>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD> + EndlessReadTarget,
    TO: WriteTarget<TransmittedWord = WORD>,
{
    /// Perform the next write of a double-buffered sequence
    pub fn write_next<BUF: WriteTarget<TransmittedWord = WORD>>(
        mut self,
        mut buf: BUF,
    ) -> DoubleBuffering<CH1, CH2, FROM, TO, WriteNext<BUF>> {
        // Make sure that memory contents reflect what the user intended.
        // TODO: How much of the following is necessary?
        cortex_m::asm::dsb();
        compiler_fence(Ordering::SeqCst);

        // Configure the _other_ DMA channel, but do not start it yet.
        if self.second_ch {
            self.ch
                .0
                .config(&self.from, &mut buf, self.pace, None, false);
        } else {
            self.ch
                .1
                .config(&self.from, &mut buf, self.pace, None, false);
        }

        // Chain the first channel to the second.
        if self.second_ch {
            self.ch.1.set_chain_to_enabled(&mut self.ch.0);
        } else {
            self.ch.0.set_chain_to_enabled(&mut self.ch.1);
        }

        DoubleBuffering {
            ch: self.ch,
            from: self.from,
            to: self.to,
            pace: self.pace,
            state: WriteNext(buf),
            second_ch: self.second_ch,
        }
    }
}

impl<CH1, CH2, FROM, TO, NEXT, WORD> DoubleBuffering<CH1, CH2, FROM, TO, ReadNext<NEXT>>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD>,
    TO: WriteTarget<TransmittedWord = WORD> + EndlessWriteTarget,
    NEXT: ReadTarget<ReceivedWord = WORD>,
{
    /// Block until the the transfer is complete
    pub fn wait(self) -> (FROM, DoubleBuffering<CH1, CH2, NEXT, TO, ()>) {
        while !self.is_done() {}

        // Make sure that memory contents reflect what the user intended.
        cortex_m::asm::dsb();
        compiler_fence(Ordering::SeqCst);

        // Invert second_ch as now the other channel is the "active" channel.
        (
            self.from,
            DoubleBuffering {
                ch: self.ch,
                from: self.state.0,
                to: self.to,
                pace: self.pace,
                state: (),
                second_ch: !self.second_ch,
            },
        )
    }
}

impl<CH1, CH2, FROM, TO, NEXT, WORD> DoubleBuffering<CH1, CH2, FROM, TO, WriteNext<NEXT>>
where
    CH1: SingleChannel,
    CH2: SingleChannel,
    FROM: ReadTarget<ReceivedWord = WORD> + EndlessReadTarget,
    TO: WriteTarget<TransmittedWord = WORD>,
    NEXT: WriteTarget<TransmittedWord = WORD>,
{
    /// Block until transfer is complete
    pub fn wait(self) -> (TO, DoubleBuffering<CH1, CH2, FROM, NEXT, ()>) {
        while !self.is_done() {}

        // Make sure that memory contents reflect what the user intended.
        cortex_m::asm::dsb();
        compiler_fence(Ordering::SeqCst);

        // Invert second_ch as now the other channel is the "active" channel.
        (
            self.to,
            DoubleBuffering {
                ch: self.ch,
                from: self.from,
                to: self.state.0,
                pace: self.pace,
                state: (),
                second_ch: !self.second_ch,
            },
        )
    }
}

/*

DoubleBuffered<(CH1, CH2), RX, TX, ()> {
    config(...) -> SingleBufferedConfig;
    is_done() -> bool
    read_next(BUF) -> DoubleReadBuffer<CH, RX, TX, BUF>
    write_next(BUF) -> DoubleWriteBuffer<CH, RX, TX, BUF>
    wait() -> ((CH1, CH2), RX, TX)
}
DoubleBuffered<CH, RX, TX, ReadNext<RX2>> {
    is_done() -> bool
    wait() -> (DoubleBuffered<CH, RX2, TX>, RX)
}
DoubleBuffered<CH, RX, TX, WriteNext<TX2>> {
    is_done() -> bool
    wait() -> (DoubleBuffered<CH, RX2, TX>, RX)
}

*/