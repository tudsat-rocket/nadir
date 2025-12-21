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

    fn draw_peers(&mut self, ui: &mut egui::Ui, system: &System) {
        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.weak("🖧 Links");
        });

        for (i, (info, _)) in system.channels().into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(5.0);
                match info.details() {
                    ChannelDetails::TcpClient { server_addr } => {
                        ui.horizontal(|ui| {
                            ui.weak("TCP");
                            ui.label(format!("{server_addr}"));
                        });
                    }
                    ChannelDetails::UdpServer {
                        server_addr,
                        peer_addr,
                    } => {
                        ui.horizontal(|ui| {
                            ui.weak("UDP");
                            ui.label(format!("{server_addr}"));
                            ui.weak("↔");
                            ui.label(format!("{peer_addr}"));
                        });
                    }
                    ChannelDetails::SerialPort { path, baud_rate: _ } => {
                        ui.horizontal(|ui| {
                            ui.weak("USB");
                            ui.label(format!("{path}"));
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

    fn draw_link_quality(&mut self, ui: &mut egui::Ui, mut lq: Option<f32>) -> egui::Response {
        lq = lq.and_then(|lq| lq.is_normal().then_some(lq));

        let color = match lq {
            Some(lq) if lq > 0.9 => COLOR_INDICATOR_GOOD,
            Some(lq) if lq > 0.5 => COLOR_INDICATOR_WARNING,
            Some(_) => COLOR_INDICATOR_LIMITS,
            None => ui.visuals().weak_text_color(),
        };

        ui.colored_label(color, format!("{:.0}%", 100.0 * lq.unwrap_or(0.0)))
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

        let (response, painter) =
            ui.allocate_painter(Vec2::new(ui.available_width(), 150.0), Sense::empty());
        let rect = response.rect.shrink(20.0);

        let icon_font = FontId::proportional(18.0);
        let weak = ui.visuals().weak_text_color();
        let align = Align2::CENTER_CENTER;
        painter.text(rect.left_center(), align, "🖳", icon_font.clone(), weak);

        if radio_status.is_some() {
            painter.text(rect.center(), align, "📡", icon_font.clone(), weak);
        }

        painter.text(rect.right_center(), align, system.icon(), icon_font, weak);

        if radio_status.is_some() {
            self.draw_link_lines(
                &painter,
                rect.left_center() + Vec2::new(20.0, 0.0),
                rect.center() - Vec2::new(20.0, 0.0),
                (local_uplink_quality, local_downlink_quality),
                false,
            );
            self.draw_link_lines(
                &painter,
                rect.center() + Vec2::new(20.0, 0.0),
                rect.right_center() - Vec2::new(20.0, 0.0),
                (remote_uplink_quality, remote_downlink_quality),
                true,
            );
        } else {
            self.draw_link_lines(
                &painter,
                rect.left_center() + Vec2::new(20.0, 0.0),
                rect.right_center() - Vec2::new(20.0, 0.0),
                (local_uplink_quality, local_downlink_quality),
                false,
            );
        }

        let local_link_target = if radio_status.is_some() {
            rect.center()
        } else {
            rect.right_center()
        };

        ui.style_mut().spacing.item_spacing = Vec2::new(5.0, 2.0);

        let local_center = rect.left_center().lerp(local_link_target, 0.5);
        let s = Vec2::new(90.0, 20.0);

        for (sign, lq) in [(-1.0, local_uplink_quality), (1.0, local_downlink_quality)] {
            ui.place(
                Rect::from_center_size(local_center + Vec2::new(0.0, 20.0 * sign), s),
                |ui: &mut egui::Ui| self.draw_link_quality(ui, lq),
            );
        }

        for (sign, pkts) in [(-1.0, up_packets), (1.0, down_packets)] {
            ui.place(
                Rect::from_center_size(local_center + Vec2::new(0.0, 42.0 * sign), s),
                |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{pkts:>5.1}"));
                        ui.weak("pkt/s ");
                    })
                    .response
                },
            );
        }

        for (sign, data) in [(-1.0, up_data), (1.0, down_data)] {
            ui.place(
                Rect::from_center_size(local_center + Vec2::new(0.0, 62.0 * sign), s),
                |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{:>5.2}", data / 1024.0));
                        ui.weak("KiB/s");
                    })
                    .response
                },
            );
        }

        if let Some(radio_status) = radio_status {
            let radio_center = rect.center().lerp(rect.right_center(), 0.5);

            let uplink_rssi = radio_status.remrssi as i8;
            let uplink_snr = uplink_rssi - (radio_status.remnoise as i8);
            let downlink_rssi = radio_status.rssi as i8;
            let downlink_snr = downlink_rssi - (radio_status.noise as i8);

            for (sign, lq) in [
                (-1.0, remote_uplink_quality),
                (1.0, remote_downlink_quality),
            ] {
                ui.place(
                    Rect::from_center_size(radio_center + Vec2::new(0.0, 20.0 * sign), s),
                    |ui: &mut egui::Ui| self.draw_link_quality(ui, lq),
                );
            }

            for (sign, rssi) in [(-1.0, uplink_rssi), (1.0, downlink_rssi)] {
                ui.place(
                    Rect::from_center_size(radio_center + Vec2::new(0.0, 42.0 * sign), s),
                    |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            ui.weak("RSSI:");
                            ui.monospace(format!("{rssi:+>3.0}"));
                            ui.weak("dBm");
                        })
                        .response
                    },
                );
            }

            for (sign, snr) in [(-1.0, uplink_snr), (1.0, downlink_snr)] {
                ui.place(
                    Rect::from_center_size(radio_center + Vec2::new(0.0, 62.0 * sign), s),
                    |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            ui.weak("SNR: ");
                            ui.monospace(format!("{snr:+>3.0}"));
                            ui.weak("dB");
                        })
                        .response
                    },
                );
            }
        }

        ui.separator();

        self.draw_peers(ui, &system);
    }
}
