use core::System;
use core::mav::ChannelDetails;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use mavspec::rust::dialects::common::messages::{LinkNodeStatus, RadioStatus};

use crate::{
    colors::{COLOR_INDICATOR_GOOD, COLOR_INDICATOR_LIMITS, COLOR_INDICATOR_WARNING, readable},
    panes::PaneUi,
};

/// The figures the compact summary shows in place of the link diagram.
struct LinkSummary {
    up_packets: f32,
    down_packets: f32,
    up_data: f32,
    down_data: f32,
    uplink_quality: Option<f32>,
    downlink_quality: Option<f32>,
    radio_status: Option<RadioStatus>,
}

pub struct LinksPane {}

impl LinksPane {
    /// Width the link diagram needs before it stops being readable. Below this the pane falls back
    /// to a numbers-only summary; below [`Self::COMPACT_MIN_WIDTH`] callers should drop the pane.
    pub const FULL_MIN_WIDTH: f32 = 285.0;
    pub const COMPACT_MIN_WIDTH: f32 = 130.0;

    pub fn new(_ctx: &egui::Context) -> Self {
        Self {}
    }

    fn draw_peers(&mut self, ui: &mut egui::Ui, system: &System) {
        ui.weak("🖧 Links");

        for (info, _) in system.channels() {
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
                        ui.label(path.clone());
                    });
                }
                _ => {
                    tracing::warn!("unimplemented channelinfo");
                }
            }
        }
    }

    fn draw_link_lines(
        &mut self,
        painter: &egui::Painter,
        ink: Color32,
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

        let uplink_stroke = Stroke::new(1.0_f32, ink.gamma_multiply(0.2 + 0.8 * lq_uplink));
        let downlink_stroke = Stroke::new(1.0_f32, ink.gamma_multiply(0.2 + 0.8 * lq_downlink));

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

    /// Transports only, no addresses: at compact widths the address is the first thing to become
    /// unreadable, while "UDP" or "USB" still answers how the vehicle is attached.
    fn draw_peers_compact(&mut self, ui: &mut egui::Ui, system: &System) {
        let mut transports: Vec<&str> = system
            .channels()
            .iter()
            .map(|(info, _)| match info.details() {
                ChannelDetails::TcpClient { .. } | ChannelDetails::TcpServer { .. } => "TCP",
                ChannelDetails::UdpClient { .. } | ChannelDetails::UdpServer { .. } => "UDP",
                ChannelDetails::SerialPort { .. } => "USB",
                _ => "?",
            })
            .collect();
        transports.sort_unstable();
        transports.dedup();

        ui.horizontal(|ui| {
            ui.weak("🖧");
            ui.weak(transports.join(" "));
        });
    }

    /// Numbers-only stand-in for the diagram, in the same uplink-over-downlink order, for zones too
    /// narrow to draw the link graph in.
    fn draw_summary(&mut self, ui: &mut egui::Ui, links: &LinkSummary) {
        ui.add_space(2.0);

        // The data-rate column is the first thing to go; packet rate and link quality are what the
        // zone exists for.
        let with_data_rate = ui.available_width() >= 200.0;

        egui::Grid::new("links_summary")
            .num_columns(if with_data_rate { 4 } else { 3 })
            .striped(true)
            .spacing(Vec2::new(6.0, 2.0))
            .show(ui, |ui| {
                ui.label("");
                ui.weak("LQ");
                ui.weak("pkt/s");
                if with_data_rate {
                    ui.weak("KiB/s");
                }
                ui.end_row();

                // Same up/down glyph pair the sidebar uses for its data rates; B612 has no
                // triangles, so anything else here renders as tofu.
                for (arrow, lq, packets, data) in [
                    ("⏫", links.uplink_quality, links.up_packets, links.up_data),
                    (
                        "⏬",
                        links.downlink_quality,
                        links.down_packets,
                        links.down_data,
                    ),
                ] {
                    ui.weak(arrow);
                    self.draw_link_quality(ui, lq);
                    ui.monospace(format!("{packets:>5.1}"));
                    if with_data_rate {
                        ui.monospace(format!("{:>5.2}", data / 1024.0));
                    }
                    ui.end_row();
                }
            });

        // The radio's own view of the link: remote RSSI over local RSSI, the pair the diagram
        // would label per direction.
        if let Some(radio) = &links.radio_status {
            ui.horizontal(|ui| {
                ui.weak("📡");
                ui.monospace(format!("{:>+4}", radio.remrssi as i8));
                ui.weak("/");
                ui.monospace(format!("{:>+4}", radio.rssi as i8));
                ui.weak("dBm");
            });
        }
    }

    fn draw_link_quality(&mut self, ui: &mut egui::Ui, mut lq: Option<f32>) -> egui::Response {
        lq = lq.and_then(|lq| lq.is_normal().then_some(lq));

        let color = match lq {
            Some(lq) if lq > 0.9 => readable(COLOR_INDICATOR_GOOD, ui.visuals()),
            Some(lq) if lq > 0.5 => readable(COLOR_INDICATOR_WARNING, ui.visuals()),
            Some(_) => readable(COLOR_INDICATOR_LIMITS, ui.visuals()),
            None => ui.visuals().weak_text_color(),
        };

        ui.colored_label(color, format!("{:.0}%", 100.0 * lq.unwrap_or(0.0)))
    }
}

impl PaneUi for LinksPane {
    fn system_ui(&mut self, ui: &mut egui::Ui, system: System) {
        let compact = ui.available_width() < Self::FULL_MIN_WIDTH;

        egui::TopBottomPanel::bottom(egui::Id::new("links_channels_panel"))
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                ui.separator();
                if compact {
                    self.draw_peers_compact(ui, &system);
                } else {
                    self.draw_peers(ui, &system);
                }
            });

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

        let radio_status = system.last_message::<RadioStatus>().ok();

        let local_uplink_quality = system.last_message::<LinkNodeStatus>().ok().map(|lns| {
            (lns.messages_received as f32) / ((lns.messages_received + lns.messages_lost) as f32)
        });
        let local_downlink_quality = Some(1.0 - down_packet_loss);

        let remote_uplink_quality = radio_status
            .as_ref()
            .map(|rs| 1.0 - f32::from(rs.fixed) / 100.0);
        let remote_downlink_quality = radio_status
            .as_ref()
            .map(|rs| 1.0 - f32::from(rs.rxerrors) / 100.0);

        if compact {
            self.draw_summary(
                ui,
                &LinkSummary {
                    up_packets,
                    down_packets,
                    up_data,
                    down_data,
                    uplink_quality: local_uplink_quality,
                    downlink_quality: local_downlink_quality,
                    radio_status,
                },
            );
            return;
        }

        // In the fixed status bar this pane gets less than its natural height; tighten the row
        // spacing and drop the data-rate rows instead of clipping.
        let diagram_h = ui.available_height().min(150.0);
        let compact = diagram_h < 130.0;
        let (off_quality, off_packets, off_data) = if compact {
            (14.0, 34.0, None)
        } else {
            (20.0, 42.0, Some(62.0))
        };

        let (response, painter) =
            ui.allocate_painter(Vec2::new(ui.available_width(), diagram_h), Sense::empty());
        let rect = response.rect.shrink(10.0);

        let icon_font = FontId::proportional(18.0);
        let weak = ui.visuals().weak_text_color();
        let ink = ui.visuals().strong_text_color();
        let align = Align2::CENTER_CENTER;
        painter.text(rect.left_center(), align, "🖳", icon_font.clone(), weak);

        if radio_status.is_some() {
            painter.text(rect.center(), align, "📡", icon_font.clone(), weak);
        }

        painter.text(rect.right_center(), align, system.icon(), icon_font, weak);

        if radio_status.is_some() {
            self.draw_link_lines(
                &painter,
                ink,
                rect.left_center() + Vec2::new(20.0, 0.0),
                rect.center() - Vec2::new(20.0, 0.0),
                (local_uplink_quality, local_downlink_quality),
                false,
            );
            self.draw_link_lines(
                &painter,
                ink,
                rect.center() + Vec2::new(20.0, 0.0),
                rect.right_center() - Vec2::new(20.0, 0.0),
                (remote_uplink_quality, remote_downlink_quality),
                true,
            );
        } else {
            self.draw_link_lines(
                &painter,
                ink,
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
                Rect::from_center_size(local_center + Vec2::new(0.0, off_quality * sign), s),
                |ui: &mut egui::Ui| self.draw_link_quality(ui, lq),
            );
        }

        for (sign, pkts) in [(-1.0, up_packets), (1.0, down_packets)] {
            ui.place(
                Rect::from_center_size(local_center + Vec2::new(0.0, off_packets * sign), s),
                |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{pkts:>5.1}"));
                        ui.weak("pkt/s ");
                    })
                    .response
                },
            );
        }

        if let Some(off_data) = off_data {
            for (sign, data) in [(-1.0, up_data), (1.0, down_data)] {
                ui.place(
                    Rect::from_center_size(local_center + Vec2::new(0.0, off_data * sign), s),
                    |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{:>5.2}", data / 1024.0));
                            ui.weak("KiB/s");
                        })
                        .response
                    },
                );
            }
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
                    Rect::from_center_size(radio_center + Vec2::new(0.0, off_quality * sign), s),
                    |ui: &mut egui::Ui| self.draw_link_quality(ui, lq),
                );
            }

            for (sign, rssi) in [(-1.0, uplink_rssi), (1.0, downlink_rssi)] {
                ui.place(
                    Rect::from_center_size(radio_center + Vec2::new(0.0, off_packets * sign), s),
                    |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            ui.weak("RSSI:");
                            ui.monospace(format!("{rssi:>+3.0}"));
                            ui.weak("dBm");
                        })
                        .response
                    },
                );
            }

            if let Some(off_data) = off_data {
                for (sign, snr) in [(-1.0, uplink_snr), (1.0, downlink_snr)] {
                    ui.place(
                        Rect::from_center_size(radio_center + Vec2::new(0.0, off_data * sign), s),
                        |ui: &mut egui::Ui| {
                            ui.horizontal(|ui| {
                                ui.weak("SNR: ");
                                ui.monospace(format!("{snr:>+3.0}"));
                                ui.weak("dB");
                            })
                            .response
                        },
                    );
                }
            }
        }
    }
}
