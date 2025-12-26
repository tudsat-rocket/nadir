use core::f32;
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct LinkStats {
    rx_last_1s: VecDeque<(Instant, usize)>,
}

#[derive(Clone, Debug, Default)]
pub struct ChannelStats {
    tx_last_1s: VecDeque<(Instant, usize)>,
    rx_last_1s: VecDeque<(Instant, u8, usize)>,
}

impl LinkStats {
    pub fn received_packet_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.rx_last_1s.len() as f32
    }

    pub fn received_data_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.rx_last_1s.iter().map(|p| p.1).sum::<usize>() as f32
    }

    fn truncate_to_1s(&mut self) {
        while self
            .rx_last_1s
            .front()
            .is_some_and(|(t, ..)| t.elapsed().as_secs_f32() > 1.0)
        {
            let _ = self.rx_last_1s.pop_front();
        }
    }

    pub fn push_received(&mut self, len: usize) {
        self.rx_last_1s.push_back((Instant::now(), len));
        self.truncate_to_1s();
    }
}

impl ChannelStats {
    pub fn packet_loss(&mut self) -> f32 {
        self.truncate_to_1s();

        let Some(mut p) = self.rx_last_1s.front() else {
            return 0.0;
        };

        let mut missed: u64 = 0;
        let mut total: u64 = 0;
        for p2 in self.rx_last_1s.iter().skip(1) {
            let diff = p2.1.wrapping_sub(p.1);
            missed += u64::max(u64::from(diff), 1) - 1;
            total += u64::from(diff);
            p = p2;
        }

        if total == 0 {
            return 0.0;
        }

        (missed as f32) / (total as f32)
    }

    pub fn received_packet_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.rx_last_1s.len() as f32
    }

    pub fn received_data_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.rx_last_1s.iter().map(|p| p.2).sum::<usize>() as f32
    }

    pub fn sent_packet_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.tx_last_1s.len() as f32
    }

    pub fn sent_data_rate(&mut self) -> f32 {
        self.truncate_to_1s();
        self.tx_last_1s.iter().map(|p| p.1).sum::<usize>() as f32
    }

    fn truncate_to_1s(&mut self) {
        while self
            .tx_last_1s
            .front()
            .is_some_and(|(t, ..)| t.elapsed().as_secs_f32() > 1.0)
        {
            let _ = self.tx_last_1s.pop_front();
        }

        while self
            .rx_last_1s
            .front()
            .is_some_and(|(t, ..)| t.elapsed().as_secs_f32() > 1.0)
        {
            let _ = self.rx_last_1s.pop_front();
        }
    }

    pub fn push_sent(&mut self, len: usize) {
        self.tx_last_1s.push_back((Instant::now(), len));
        self.truncate_to_1s();
    }

    pub fn push_received(&mut self, seq: u8, len: usize) {
        self.rx_last_1s.push_back((Instant::now(), seq, len));
        self.truncate_to_1s();
    }
}
