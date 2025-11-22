use eframe::egui;
use maviola::core::io::ChannelDetails;

use crate::{panes::TreeBehavior, views::View};

pub struct LinksPane {}

impl LinksPane {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {}
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        ui.add_space(5.0);

        for (i, (info, mut stats)) in system.channels().into_iter().enumerate() {
            if i != 0 {
                ui.separator();
            }

            let info_string = match info.details() {
                ChannelDetails::TcpClient { server_addr } => format!("tcp:{server_addr}"),
                ChannelDetails::UdpServer {
                    server_addr,
                    peer_addr,
                } => format!("udp:{server_addr} (peer: {peer_addr})"),
                _ => {
                    tracing::warn!("unimplemented channelinfo");
                    "".to_owned()
                }
            };

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("🖧");
                ui.label(info_string);
            });

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("☠");
                ui.monospace(format!("{:>4.1}", stats.packet_loss() * 100.0));
                ui.label("% lost");
            });

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("⏬");
                ui.monospace(format!("{:>3.0}", stats.incoming_packet_rate()));
                ui.label("pkt/s ");
                ui.monospace(format!("{:>5.2}", stats.incoming_data_rate() / 1024.0));
                ui.label("KiB/s");
            });

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("⏫");
                ui.monospace(format!("{:>3.0}", stats.outgoing_packet_rate()));
                ui.label("pkt/s ");
                ui.monospace(format!("{:>5.2}", stats.outgoing_data_rate() / 1024.0));
                ui.label("KiB/s");
            });
        }
    }
}
