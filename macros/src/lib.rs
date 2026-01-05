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

    let protocol = mavspec::definitions::protocol();
    let common = protocol.get_dialect_by_name("common").unwrap();
    let dialect = protocol.get_dialect_by_name(&dialect_name).unwrap();

    let inner_message_match_arms: Vec<_> = dialect
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
        .map(|msg_spec| {
            let lower_case = msg_spec.name().to_lowercase();
            let type_name: String = lower_case
                .split('_')
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            let type_ident = format_ident!("{}", type_name);

            quote! {
                Self::#type_ident(inner) => inner.insert(conn, system_id, component_id)
            }
        })
        .collect();

    // We derive `MessageExt` for the main enum for each dialect as well. We only use the insertion
    // functionality of that right now, maybe that should be refactored into a separate trait.
    let dialect_impl = quote! {
        impl MessageExt for #dialect_type {
            fn table(&self) -> &str {
                unreachable!()
            }

            fn rows(&self) -> &[&str] {
                unreachable!()
            }

            fn insert<'a>(
                &'a self,
                conn: &rusqlite::Connection,
                system_id: u8,
                component_id: u8
            ) -> Result<(), rusqlite::Error> {
                match self {
                    #(#inner_message_match_arms),* ,
                    _ => unimplemented!()
                }
            }

            fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error> {
                unreachable!()
            }
        }
    };

    let message_impls: Vec<_> = dialect
        .messages()
        .into_iter()
        .filter(|msg_spec| {
            dialect.name() == "common" || common.get_message_by_id(msg_spec.id()).is_none()
        })
        .map(|msg_spec| {
            let lower_case = msg_spec.name().to_lowercase();
            let type_name: String = lower_case
                .split('_')
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            let type_ident = format_ident!("{}", type_name);

            let table_name = format!("messages_{lower_case}");
            let row_names: Vec<_> = msg_spec
                .fields()
                .iter()
                .map(|f| match f.name() {
                    "index" => "index_".to_owned(),
                    "type" => "type_".to_owned(),
                    n => n.to_lowercase(),
                })
                .collect();

            let var_names: Vec<_> = msg_spec
                .fields()
                .iter()
                .map(|f| {
                    match f.name() {
                        "I2Cerr" => "i2_cerr".to_owned(),
                        "EAS2TAS" => "eas2tas".to_owned(),
                        "type" => "type_".to_owned(),
                        n if n.chars().any(char::is_uppercase)
                            => n.to_case(Case::Snake),
                        n => n.to_owned(),
                    }
                })
            .collect();

            let param_names: Vec<_> = var_names
                .iter()
                .map(|varname| format!(":{varname}"))
                .collect();

            let param_assignments: Vec<_> = msg_spec
                .fields()
                .iter()
                .zip(var_names.iter())
                .map(|(f, varname)| {
                    let param_name = format!(":{varname}");
                    let var_ident = format_ident!("{}", varname);

                    match (f.r#type(), f.r#enum()) {
                        (MavType::Array(_, _), Some(_)) if is_field_bitmask(f) => quote!{
                            #param_name: self.#var_ident.iter().map(|x| x.bits().to_be_bytes()).flatten().collect::<Vec<_>>()
                        },
                        (MavType::Array(_, _), Some(_)) => quote!{
                            #param_name: self.#var_ident.iter().map(|x| x.value().to_be_bytes()).flatten().collect::<Vec<_>>()
                        },
                        (MavType::Array(_, _), None) => quote!{
                            #param_name: self.#var_ident.iter().map(|x| x.to_be_bytes()).flatten().collect::<Vec<_>>()
                        },
                        (_, Some(_)) if is_field_bitmask(f) => quote!{ #param_name: self.#var_ident.bits() },
                        (_, Some(_)) => quote!{ #param_name: self.#var_ident.value() },
                        (MavType::Float, _) => quote!{ #param_name: self.#var_ident.to_bits() },
                        (MavType::Double, _) => quote!{ #param_name: self.#var_ident.to_bits() },
                        _ => quote!{ #param_name: self.#var_ident }
                    }
                })
                .collect();

            let value_assignments: Vec<_> = msg_spec
                .fields()
                .iter()
                .enumerate()
                .zip(var_names.iter())
                .map(|((i, f), varname)| {
                    let var_ident = format_ident!("{}", varname);

                    match (f.r#type(), f.r#enum()) {
                        // TODO: Reading of arrays of enums/bitmasks not implemented so far.
                        // We don't currently use any messages which need this.
                        (MavType::Array(_, _), Some(_)) if is_field_bitmask(f) => quote! {
                            #var_ident: {
                                unimplemented!()
                            }
                        },
                        (MavType::Array(_, _), Some(_)) => quote! {
                            #var_ident: {
                                unimplemented!()
                            }
                        },
                        (MavType::Array(element_type, _len), None) => {
                            let size = element_type.size();
                            let rust_type = format_ident!("{}", element_type.rust_type());

                            quote! {
                                #var_ident: {
                                    let blob = row.get::<usize, Vec<u8>>(#i)?;
                                    let vec: Vec<_> = blob.chunks(#size).map(|chunk| {
                                        let chunk_array: [u8; #size] = chunk.try_into().unwrap();
                                        #rust_type::from_be_bytes(chunk_array)
                                    }).collect();
                                    vec.try_into().unwrap()
                                }
                            }
                        }
                        (_, Some(e)) if is_field_bitmask(f) => {
                            let enum_type = e.to_case(Case::Pascal);
                            let enum_ident = format_ident!("{}", enum_type);

                            quote! {
                                #var_ident: #dialect_mod::enums::#enum_ident::from_bits(row.get::<usize, _>(#i)?).unwrap()
                            }
                        }
                        (_, Some("MAV_REMOTE_LOG_DATA_BLOCK_COMMANDS")) => {
                            quote! { #var_ident: row.get::<usize, u32>(#i)?.try_into().unwrap() }
                        }
                        (_, Some("MAV_CMD")) => {
                            quote! { #var_ident: row.get::<usize, u16>(#i)?.try_into().unwrap() }
                        }
                        (_, Some(_)) => {
                            quote! { #var_ident: row.get::<usize, u8>(#i)?.try_into().unwrap() }
                        }
                        (MavType::Float, _) => quote! { #var_ident: f32::from_bits(row.get::<usize, u32>(#i)?) },
                        (MavType::Double, _) => quote! { #var_ident: f64::from_bits(row.get::<usize, u64>(#i)?) },
                        _ => quote! { #var_ident: row.get(#i)? }
                    }
                })
                .collect();

            let row_names_list = row_names.join(",");
            let param_names_list = param_names.join(",");

            quote! {
                impl MessageExt for #dialect_mod::messages::#type_ident {
                    fn table(&self) -> &str {
                        #table_name
                    }

                    fn rows(&self) -> &[&str] {
                        &[
                            #(#row_names),*
                        ]
                    }

                    fn insert<'a>(
                        &'a self,
                        conn: &rusqlite::Connection,
                        system_id: u8,
                        component_id: u8
                    ) -> Result<(), rusqlite::Error> {
                        let query = format!(
                            "INSERT INTO {}
                                (received_at, system_id, component_id, {})
                                VALUES (:received_at, :system_id, :component_id, {})",
                            #table_name,
                            #row_names_list,
                            #param_names_list,
                        );

                        conn.execute(
                            &query,
                            rusqlite::named_params! {
                                ":received_at": chrono::Utc::now(),
                                ":system_id": system_id,
                                ":component_id": component_id,
                                #(#param_assignments),*
                            }
                        )?;

                        Ok(())
                    }

                    #[allow(unreachable_code)] // see TODO above
                    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error> {
                        Ok(Self {
                            #(#value_assignments),*
                        })
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
    )
}
