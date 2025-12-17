use core::System;

use eframe::egui;
use egui::{Align, Align2, Color32, FontId, Layout, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use maviola::core::io::ChannelDetails;

use crate::{
    colors::{COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING},
    panes::TreeBehavior,
    views::View,
};

pub struct LinksPane {}

impl LinksPane {
    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    // TODO: add peer information back in
    #[allow(dead_code)]
    fn draw_peers(&mut self, ui: &mut egui::Ui, system: &System) {
        for (i, (info, _)) in system.channels().into_iter().enumerate() {
            if i != 0 {
                ui.separator();
            }

            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.weak("🖧");
                match info.details() {
                    ChannelDetails::TcpClient { server_addr } => {
                        ui.label(format!("tcp:{server_addr}"));
                    }
                    ChannelDetails::UdpServer {
                        server_addr,
                        peer_addr,
                    } => {
                        ui.vertical(|ui| {
                            ui.label(format!("udp:{server_addr}"));
                            ui.weak(format!("(peer: {peer_addr})"));
                        });
                    }
                    ChannelDetails::SerialPort { path, baud_rate } => {
                        ui.vertical(|ui| {
                            ui.label(format!("serial:{path}"));
                            ui.weak(format!("(baud rate: {baud_rate})"));
                        });
                    }
                    _ => {
                        tracing::warn!("unimplemented channelinfo");
                    }
                }
            });
        }
    }

    fn draw_link_lines(
        &mut self,
        painter: &egui::Painter,
        a: Pos2,
        b: Pos2,
        link_qualities: (Option<f32>, Option<f32>),
        dashed: bool,
    ) {
        let points_uplink = vec![
            a + Vec2::new(0.0, -5.0),
            b + Vec2::new(0.0, -5.0),
            b + Vec2::new(0.0, -5.0) + Vec2::new(-5.0, -5.0),
        ];

        let points_downlink = vec![
            b + Vec2::new(0.0, 5.0),
            a + Vec2::new(0.0, 5.0),
            a + Vec2::new(0.0, 5.0) + Vec2::new(5.0, 5.0),
        ];

        let lq_uplink = link_qualities
            .0
            .and_then(|lq| lq.is_normal().then_some(lq))
            .unwrap_or(0.0);
        let lq_downlink = link_qualities
            .1
            .and_then(|lq| lq.is_normal().then_some(lq))
            .unwrap_or(0.0);

        let uplink_stroke = Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.2 + 0.8 * lq_uplink));
        let downlink_stroke =
            Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.2 + 0.8 * lq_downlink));

        if dashed {
            painter.add(Shape::dashed_line(&points_uplink, uplink_stroke, 4.0, 2.0));
            painter.add(Shape::dashed_line(
                &points_downlink,
                downlink_stroke,
                4.0,
                2.0,
            ));
        } else {
            painter.line(points_uplink, uplink_stroke);
            painter.line(points_downlink, downlink_stroke);
        }
    }

    fn draw_link_quality(&mut self, ui: &mut egui::Ui, mut lq: Option<f32>) {
        lq = lq.and_then(|lq| lq.is_normal().then_some(lq));

        let color = match lq {
            Some(lq) if lq > 0.9 => COLOR_INDICATOR_GOOD,
            Some(lq) if lq > 0.5 => COLOR_INDICATOR_WARNING,
            Some(_) => COLOR_INDICATOR_LIMITS,
            None => ui.visuals().weak_text_color(),
        };

        ui.colored_label(color, format!("{:.0}%", 100.0 * lq.unwrap_or(0.0)));
    }

    pub fn pane_ui(&mut self, ui: &mut egui::Ui, behavior: &mut TreeBehavior) {
        let View::System(system_id) = behavior.active_view else {
            return;
        };

        let Some(system) = behavior.core.system(system_id) else {
            return;
        };

        let up_packets: f32 = system
            .channels()
            .iter_mut()
            .map(|(_channel, stats)| stats.sent_packet_rate())
            .sum();

        let up_data: f32 = system
            .channels()
            .iter_mut()
            .map(|(_channel, stats)| stats.sent_data_rate())
            .sum();

        let down_packets: f32 = system
            .channels()
            .iter_mut()
            .map(|(_channel, stats)| stats.received_packet_rate())
            .sum();

        let down_data: f32 = system
            .channels()
            .iter_mut()
            .map(|(_channel, stats)| stats.received_data_rate())
            .sum();

        let down_packet_loss = system
            .channels()
            .iter_mut()
            .map(|(_channel, stats)| stats.packet_loss() * stats.received_packet_rate())
            .sum::<f32>()
            / down_packets;

        let radio_status = system.last_radio_status_for_system().ok().flatten();

        let local_uplink_quality = system
            .last_link_node_status_for_system()
            .ok()
            .flatten()
            .map(|lns| {
                (lns.messages_received as f32)
                    / ((lns.messages_received + lns.messages_lost) as f32)
            });
        let local_downlink_quality = Some(1.0 - down_packet_loss);

        let remote_uplink_quality = radio_status
            .as_ref()
            .map(|rs| 1.0 - f32::from(rs.fixed) / 100.0);
        let remote_downlink_quality = radio_status
            .as_ref()
            .map(|rs| 1.0 - f32::from(rs.rxerrors) / 100.0);

        let (response, painter) = ui.allocate_painter(
            Vec2::new(ui.available_width(), ui.available_height()),
            Sense::empty(),
        );

        let inset = response.rect.shrink(20.0);

        painter.text(
            inset.left_center(),
            Align2::CENTER_CENTER,
            "🖳",
            FontId::proportional(18.0),
            ui.visuals().weak_text_color(),
        );

        if radio_status.is_some() {
            painter.text(
                inset.center(),
                Align2::CENTER_CENTER,
                "📡",
                FontId::proportional(18.0),
                ui.visuals().weak_text_color(),
            );
        }

        painter.text(
            inset.right_center(),
            Align2::CENTER_CENTER,
            system.icon(),
            FontId::proportional(18.0),
            ui.visuals().weak_text_color(),
        );

        if radio_status.is_some() {
            self.draw_link_lines(
                &painter,
                inset.left_center() + Vec2::new(20.0, 0.0),
                inset.center() - Vec2::new(20.0, 0.0),
                (local_uplink_quality, local_downlink_quality),
                false,
            );
            self.draw_link_lines(
                &painter,
                inset.center() + Vec2::new(20.0, 0.0),
                inset.right_center() - Vec2::new(20.0, 0.0),
                (remote_uplink_quality, remote_downlink_quality),
                true,
            );
        } else {
            self.draw_link_lines(
                &painter,
                inset.left_center() + Vec2::new(20.0, 0.0),
                inset.right_center() - Vec2::new(20.0, 0.0),
                (local_uplink_quality, local_downlink_quality),
                false,
            );
        }

        let local_link_target = if radio_status.is_some() {
            inset.center()
        } else {
            inset.right_center()
        };

        ui.place(
            Rect::from_two_pos(inset.left_top(), local_link_target).shrink(20.0),
            |ui: &mut egui::Ui| {
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    self.draw_link_quality(ui, local_uplink_quality);
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.monospace(format!("{up_packets:>5.1}"));
                        ui.weak("pkt/s ");
                        ui.monospace(format!("{:>5.2}", up_data / 1024.0));
                        ui.weak("KiB/s");
                    });
                })
                .response
            },
        );

        ui.place(
            Rect::from_two_pos(inset.left_bottom(), local_link_target).shrink(20.0),
            |ui: &mut egui::Ui| {
                ui.vertical_centered(|ui| {
                    self.draw_link_quality(ui, local_downlink_quality);
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.monospace(format!("{down_packets:>5.1}"));
                        ui.weak("pkt/s ");
                        ui.monospace(format!("{:>5.2}", down_data / 1024.0));
                        ui.weak("KiB/s");
                    });
                })
                .response
            },
        );

        if let Some(radio_status) = radio_status {
            ui.place(
                Rect::from_two_pos(inset.right_top(), inset.center()).shrink(20.0),
                |ui: &mut egui::Ui| {
                    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                        let uplink_rssi = radio_status.remrssi as i8;
                        let uplink_snr = uplink_rssi - (radio_status.remnoise as i8);

                        self.draw_link_quality(ui, remote_uplink_quality);
                        ui.add_space(5.0);

                        ui.with_layout(Layout::left_to_right(Align::BOTTOM), |ui| {
                            ui.weak("RSSI");
                            ui.monospace(format!("{uplink_rssi:>3.0}"));
                            ui.weak("dBm, SNR:");
                            ui.monospace(format!("{uplink_snr:>2.0}"));
                            ui.weak("dB");
                        });
                    })
                    .response
                },
            );

            ui.place(
                Rect::from_two_pos(inset.right_bottom(), inset.center()).shrink(20.0),
                |ui: &mut egui::Ui| {
                    ui.vertical_centered(|ui| {
                        let downlink_rssi = radio_status.rssi as i8;
                        let downlink_snr = downlink_rssi - (radio_status.noise as i8);

                        self.draw_link_quality(ui, remote_downlink_quality);
                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.weak("RSSI");
                            ui.monospace(format!("{downlink_rssi:>3.0}"));
                            ui.weak("dBm, SNR:");
                            ui.monospace(format!("{downlink_snr:>2.0}"));
                            ui.weak("dB");
                        });
                    })
                    .response
                },
            );
        }
    }
}
