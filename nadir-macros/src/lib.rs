extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

use convert_case::{Case, Casing as _};
use mavinspect::protocol::{MavType, MessageField};

#[allow(dead_code)]
struct MacroInput {
    dialect_name: syn::LitStr,
    comma: syn::Token![,],
    dialect_type: syn::Path,
    comma2: syn::Token![,],
    dialect_mod: syn::Path,
}

impl syn::parse::Parse for MacroInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self {
            dialect_name: input.parse()?,
            comma: input.parse()?,
            dialect_type: input.parse()?,
            comma2: input.parse()?,
            dialect_mod: input.parse()?,
        })
    }
}

#[proc_macro]
pub fn implement_message_ext_for_dialect(args: TokenStream) -> TokenStream {
    let input = parse_macro_input!(args as MacroInput);
    let dialect_name = input.dialect_name.value();
    let dialect_type = input.dialect_type;
    let dialect_mod = input.dialect_mod;

    // rapid lives in its own crate and ships its own parsed protocol; mavspec's
    // bundled definitions only cover common, ardupilotmega, standard, minimal.
    let (protocol, common, dialect) = if dialect_name == "rapid" {
        let p = rapid_dialect::definitions::protocol();
        let c = p.get_dialect_by_canonical_name("common").unwrap();
        let d = p.get_dialect_by_canonical_name(&dialect_name).unwrap();
        (p, c, d)
    } else {
        let p = mavspec::definitions::protocol();
        let c = p.get_dialect_by_name("common").unwrap();
        let d = p.get_dialect_by_name(&dialect_name).unwrap();
        (p, c, d)
    };
    let _ = protocol;

    let filtered_messages: Vec<_> = dialect
        .messages()
        .into_iter()
        // Ardupilot modifies some messages, we ignore those changes for now
        .filter(|msg_spec| {
            dialect_name != "ardupilotmega"
                || (msg_spec.name() != "MISSION_ITEM_INT"
                    && msg_spec.name() != "MISSION_ITEM"
                    && msg_spec.name() != "COMMAND_INT"
                    && msg_spec.name() != "COMMAND_LONG"
                    && msg_spec.name() != "COMMAND_ACK"
                    && msg_spec.name() != "COMMAND_CANCEL"
                    && msg_spec.name() != "TRAJECTORY_REPRESENTATION_WAYPOINTS")
        })
        .collect();

    let type_idents: Vec<_> = filtered_messages
        .iter()
        .map(|msg_spec| message_type_ident(msg_spec.name()))
        .collect();

    let inner_store_match_arms: Vec<_> = type_idents
        .iter()
        .map(|type_ident| {
            quote! {
                Self::#type_ident(inner) => inner.store(db, system_id, component_id, received_at)
            }
        })
        .collect();

    let inner_instance_match_arms: Vec<_> = type_idents
        .iter()
        .map(|type_ident| {
            quote! {
                Self::#type_ident(inner) => inner.instance_value()
            }
        })
        .collect();

    // A decoded frame arrives as one of these enums, so the enum impl only dispatches a write to
    // its variant's concrete type. Everything read back out is read through that type.
    let dialect_impl = quote! {
        impl MessageExt for #dialect_type {
            fn rows() -> &'static [&'static str] {
                unreachable!()
            }

            fn instance_value(&self) -> Option<i64> {
                match self {
                    #(#inner_instance_match_arms),* ,
                    _ => None
                }
            }

            fn field_f64(&self, index: usize) -> Option<f64> {
                unreachable!()
            }

            fn store(
                &self,
                db: &Db,
                system_id: u8,
                component_id: u8,
                received_at: chrono::DateTime<chrono::Utc>,
            ) {
                match self {
                    #(#inner_store_match_arms),* ,
                    _ => {}
                }
            }
        }
    };

    let message_impls: Vec<_> = dialect
        .messages()
        .into_iter()
        .filter(|msg_spec| {
            // rapid lives in its own type universe (rapid-dialect generates its
            // own `common` module), so the common-dialect MessageExt impls don't
            // cover its inherited variants - emit impls for all rapid messages.
            dialect.name() == "common"
                || dialect_name == "rapid"
                || common.get_message_by_id(msg_spec.id()).is_none()
        })
        .map(|msg_spec| {
            let type_ident = message_type_ident(msg_spec.name());

            // The MAVLink field names rather than the mangled Rust ones below, since a caller
            // addressing a field by name has what the definitions say.
            let row_names: Vec<_> = msg_spec
                .fields()
                .iter()
                .map(|f| f.name().to_owned())
                .collect();

            let var_names: Vec<_> = msg_spec
                .fields()
                .iter()
                .map(|f| match f.name() {
                    "I2Cerr" => "i2_cerr".to_owned(),
                    "EAS2TAS" => "eas2tas".to_owned(),
                    "type" => "type_".to_owned(),
                    n if n.chars().any(char::is_uppercase) => n.to_case(Case::Snake),
                    n => n.to_owned(),
                })
                .collect();

            let instance_value_impl = msg_spec
                .fields()
                .iter()
                .zip(var_names.iter())
                .find(|(f, _)| f.instance())
                .map_or_else(
                    || quote! { None },
                    |(f, varname)| {
                        let var_ident = format_ident!("{}", varname);
                        match (f.r#type(), f.r#enum()) {
                            // String / array instance fields (e.g. DEBUG_VECT.name) don't
                            // map to a single i64; the per-instance breakdown for them
                            // simply collapses into the no-instance path.
                            (MavType::Array(_, _), _) => quote! { None },
                            (_, Some(_)) if is_field_bitmask(f) => quote! {
                                Some(i64::from(self.#var_ident.bits()))
                            },
                            (_, Some(_)) => quote! {
                                Some(i64::from(self.#var_ident.value()))
                            },
                            _ => quote! { Some(i64::from(self.#var_ident)) },
                        }
                    },
                );

            let instance_field_impl = msg_spec
                .fields()
                .iter()
                .zip(row_names.iter())
                .find(|(f, _)| f.instance())
                .and_then(|(f, row_name)| match f.r#type() {
                    MavType::Array(_, _) => None,
                    _ => Some(row_name.clone()),
                })
                .map_or_else(|| quote! { None }, |row_name| quote! { Some(#row_name) });

            // Each field as a plain number, indexed as `row_names` is. An array field has no
            // single number, and so nothing to plot.
            let field_value_arms: Vec<_> = msg_spec
                .fields()
                .iter()
                .enumerate()
                .zip(var_names.iter())
                .map(|((i, f), varname)| {
                    let var_ident = format_ident!("{}", varname);

                    match (f.r#type(), f.r#enum()) {
                        (MavType::Array(_, _), _) => quote! { #i => None },
                        (_, Some(_)) if is_field_bitmask(f) => quote! {
                            #i => Some(self.#var_ident.bits() as f64)
                        },
                        (_, Some(_)) => quote! { #i => Some(self.#var_ident.value() as f64) },
                        _ => quote! { #i => Some(self.#var_ident as f64) },
                    }
                })
                .collect();

            quote! {
                impl MessageExt for #dialect_mod::messages::#type_ident {
                    fn rows() -> &'static [&'static str] {
                        &[
                            #(#row_names),*
                        ]
                    }

                    fn instance_value(&self) -> Option<i64> {
                        #instance_value_impl
                    }

                    fn instance_field() -> Option<&'static str> {
                        #instance_field_impl
                    }

                    fn field_f64(&self, index: usize) -> Option<f64> {
                        match index {
                            #(#field_value_arms,)*
                            _ => None
                        }
                    }

                    fn store(
                        &self,
                        db: &Db,
                        system_id: u8,
                        component_id: u8,
                        received_at: chrono::DateTime<chrono::Utc>,
                    ) {
                        db.push(system_id, component_id, received_at, self.clone());
                    }
                }
            }
        })
        .collect();

    quote! {
        #dialect_impl
        #(#message_impls)*
    }
    .into()
}

/// The Rust type mavspec generates for a message, e.g. `GLOBAL_POSITION_INT` -> `GlobalPositionInt`.
fn message_type_ident(name: &str) -> syn::Ident {
    let type_name: String = name
        .to_lowercase()
        .split('_')
        .map(|s| {
            let (head, tail) = s.split_at(1);
            format!("{}{}", head.to_uppercase(), tail)
        })
        .collect();

    format_ident!("{}", type_name)
}

// For some reason, the bitmask flag is not set correctly for some messages, work around that.
fn is_field_bitmask(f: &MessageField) -> bool {
    matches!(
        f.r#enum().unwrap_or_default(),
        "ADSB_FLAGS"
            | "AIS_FLAGS"
            | "ATTITUDE_TARGET_TYPEMASK"
            | "CAMERA_CAP_FLAGS"
            | "CAMERA_TRACKING_TARGET_DATA"
            | "ESC_FAILURE_FLAGS"
            | "ESTIMATOR_STATUS_FLAGS"
            | "GIMBAL_DEVICE_CAP_FLAGS"
            | "GIMBAL_DEVICE_ERROR_FLAGS"
            | "GIMBAL_DEVICE_FLAGS"
            | "GIMBAL_MANAGER_CAP_FLAGS"
            | "GIMBAL_MANAGER_FLAGS"
            | "GPS_INPUT_IGNORE_FLAGS"
            | "HIGHRES_IMU_UPDATED_FLAGS"
            | "HIL_ACTUATOR_CONTROLS_FLAGS"
            | "HIL_SENSOR_UPDATED_FLAGS"
            | "HL_FAILURE_FLAG"
            | "ILLUMINATOR_ERROR_FLAGS"
            | "MAV_BATTERY_FAULT"
            | "MAV_GENERATOR_STATUS_FLAG"
            | "MAV_MODE_FLAG"
            | "MAV_MODE_PROPERTY"
            | "MAV_POWER_STATUS"
            | "MAV_PROTOCOL_CAPABILITY"
            | "MAV_SYS_STATUS_SENSOR"
            | "MAV_SYS_STATUS_SENSOR_EXTENDED"
            | "MAV_WINCH_STATUS_FLAG"
            | "POSITION_TARGET_TYPEMASK"
            | "SERIAL_CONTROL_FLAG"
            | "STORAGE_USAGE_FLAG"
            | "UTM_DATA_AVAIL_FLAGS"
            | "LIMIT_MODULE"
            | "UAVIONIX_ADSB_OUT_STATUS_FAULT"
            | "UAVIONIX_ADSB_OUT_DYNAMIC_STATE"
            | "UAVIONIX_ADSB_OUT_RF_SELECT"
            | "UAVIONIX_ADSB_OUT_RF_HEALTH"
            | "UAVIONIX_ADSB_OUT_CONTROL_STATE"
            | "UAVIONIX_ADSB_OUT_STATUS_STATE"
            | "UAVIONIX_ADSB_XBIT"
            | "UAVIONIX_ADSB_RF_HEALTH"
            | "GOPRO_HEARTBEAT_FLAGS"
            | "EKF_STATUS_FLAGS"
            | "VIDEO_STREAM_STATUS_FLAGS"
            | "PRESSURE_VESSEL_FLAG"
            | "ROCKET_CAPABILITY"
    )
}
