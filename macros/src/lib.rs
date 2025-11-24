extern crate proc_macro;

use mavinspect::protocol::MavType;
use proc_macro::TokenStream;

#[proc_macro]
pub fn generate_message_writers(_item: TokenStream) -> TokenStream {
    let protocol = mavspec::definitions::protocol();
    let dialect = protocol.get_dialect_by_name("common").unwrap();

    let mut methods: String = dialect
        .messages()
        .into_iter()
        .map(|m| {
            let lower_case = m.name().to_lowercase();
            let type_name: String = lower_case
                .split("_")
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            let fields = m
                .fields()
                .iter()
                .map(|f| match f.name() {
                    "index" => "index_".to_owned(),
                    "type" => "type_".to_owned(),
                    n => n.to_lowercase(),
                })
                .collect::<Vec<_>>()
                .join(",");
            let param_refs = m
                .fields()
                .iter()
                .enumerate()
                .map(|(i, _f)| format!("?{}", i + 4))
                .collect::<Vec<_>>()
                .join(",");
            let values = m
                .fields()
                .iter()
                .map(|f| {
                    let varname = match f.name() {
                        "type" => "type_".to_owned(),
                        "zoomLevel" => "zoom_level".to_owned(),
                        "focusLevel" => "focus_level".to_owned(),
                        n => n.to_lowercase(),
                    };

                    // TODO: for some reason, the bitflag flag is not set correctly, work
                    // around that.
                    match (f.r#type(), f.r#enum()) {
                        (MavType::Array(_, _), Some("ESC_FAILURE_FLAGS"))
                                => format!("msg.{}.iter().map(|x| x.bits().to_be_bytes()).flatten().collect::<Vec<_>>()", varname),
                        (MavType::Array(_, _), Some("MAV_CMD"))
                                => format!("msg.{}.iter().map(|x| x.value().to_be_bytes()).flatten().collect::<Vec<_>>()", varname),
                        (MavType::Array(_, _), None)
                                => format!("msg.{}.iter().map(|x| x.to_be_bytes()).flatten().collect::<Vec<_>>()", varname),
                        (_, Some("ADSB_FLAGS"
                            | "AIS_FLAGS"
                            | "ATTITUDE_TARGET_TYPEMASK"
                            | "CAMERA_CAP_FLAGS"
                            | "CAMERA_TRACKING_TARGET_DATA"
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
                            | "VIDEO_STREAM_STATUS_FLAGS")) => format!("msg.{}.bits()", varname),
                        (_, Some(_)) => format!("msg.{}.value()", varname),
                        _ => format!("msg.{}", varname)
                    }
                })
                .collect::<Vec<_>>()
                .join(",");

            format!(
                "
                pub fn write_message_{lower_case}(
                    &self,
                    msg: mavspec::rust::dialects::common::messages::{type_name},
                    frame: maviola::prelude::Frame<maviola::prelude::V2>,
                    callback: maviola::asnc::prelude::Callback<maviola::prelude::V2>,
                ) -> Result<(), DbError> {{
                    use mavspec::rust::dialects::common::*;

                    let conn = self.conn();
                    conn.busy_timeout(std::time::Duration::from_millis(10))?;

                    let system_id = frame.system_id();
                    let component_id = frame.component_id();

                    conn.execute(
                        \"INSERT INTO messages_{lower_case}
                            (received_at, system_id, component_id, {fields})
                            VALUES (?1, ?2, ?3, {param_refs})\",

                        rusqlite::params![chrono::Utc::now(), system_id, component_id, {values}]
                    )?;

                    Ok(())
                }}
                "
            )
        })
        .collect();

    let match_arms: String = dialect
        .messages()
        .into_iter()
        .map(|m| {
            let lower_case = m.name().to_lowercase();
            let type_name: String = lower_case
                .split("_")
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            format!("Common::{type_name}(inner) => self.write_message_{lower_case}(inner, frame, callback),")
        })
        .collect();

    methods.extend(
        format!(
            "
            pub fn write_common_message(
                &self,
                msg: mavspec::rust::dialects::Common,
                frame: maviola::prelude::Frame<maviola::prelude::V2>,
                callback: maviola::asnc::prelude::Callback<maviola::prelude::V2>,
            ) -> Result<(), DbError> {{
                use mavspec::rust::dialects::Common;
                match msg {{
                    {match_arms}
                }}
            }}
            "
        )
        .chars(),
    );

    let output = format!("impl Db {{\n{methods}\n}}");
    output.parse().unwrap()
}

#[proc_macro]
pub fn generate_message_readers(_item: TokenStream) -> TokenStream {
    let protocol = mavspec::definitions::protocol();
    let dialect = protocol.get_dialect_by_name("common").unwrap();

    let mut methods: String = dialect
        .messages()
        .into_iter()
        .map(|m| {
            let lower_case = m.name().to_lowercase();
            let type_name: String = lower_case
                .split("_")
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            let fields = m
                .fields()
                .iter()
                .map(|f| match f.name() {
                    "index" => "index_".to_owned(),
                    "type" => "type_".to_owned(),
                    n => n.to_lowercase(),
                })
                .collect::<Vec<_>>()
                .join(",");
            let param_refs = m
                .fields()
                .iter()
                .enumerate()
                .map(|(i, _f)| format!("?{}", i + 1))
                .collect::<Vec<_>>()
                .join(",");
            let values = m
                .fields()
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let varname = match f.name() {
                        "type" => "type_".to_owned(),
                        "zoomLevel" => "zoom_level".to_owned(),
                        "focusLevel" => "focus_level".to_owned(),
                        n => n.to_lowercase(),
                    };

                    // TODO: see above
                    match (f.r#type(), f.r#enum()) {
                        // TODO
                        (MavType::Array(_, _), Some("ESC_FAILURE_FLAGS"))
                            => format!("{varname}: Default::default()"),
                        (MavType::Array(_, _), Some("MAV_CMD"))
                            => format!("{varname}: Default::default()"),
                        (MavType::Array(element_type, len), None) => {
                            let elem_type = match &**element_type {
                                MavType::Float | MavType::Double => "0.0",
                                _ => "0",
                            };
                            format!("{varname}: [{elem_type}; {len}]")
                        }
                        (_, Some("ADSB_FLAGS"
                            | "AIS_FLAGS"
                            | "ATTITUDE_TARGET_TYPEMASK"
                            | "CAMERA_CAP_FLAGS"
                            | "CAMERA_TRACKING_TARGET_DATA"
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
                            | "VIDEO_STREAM_STATUS_FLAGS"
                            | "VIDEO_STREMA_STATUS_FLAGS")) => {
                            format!("{varname}: Default::default()")
                        }
                        (_, Some("MAV_CMD")) => {
                            format!("{varname}: row.get::<usize, u16>({i})?.try_into().unwrap()")
                        }
                        (_, Some(enum_name)) => {
                            format!("{varname}: row.get::<usize, u8>({i})?.try_into().unwrap()")
                        }
                        _ => format!("{varname}: row.get({i})?")
                    }
                })
                .collect::<Vec<_>>()
                .join(",");

            format!(
                "
                pub fn count_{lower_case}_for_system(
                    &self,
                    system_and_component_ids: (u8, u8)
                ) -> Result<usize, DbError> {{
                    let conn = self.conn();
                    conn.busy_timeout(std::time::Duration::from_millis(10))?;

                    let system_id = system_and_component_ids.0;
                    let component_id = system_and_component_ids.1;

                    let mut stmt = conn.prepare(\"
                        SELECT COUNT(*) FROM messages_{lower_case}
                        WHERE system_id=:system_id AND component_id=:component_id\")?;

                    let mut rows = stmt.query_map(&[
                        (\":system_id\", &system_id),
                        (\":component_id\", &component_id)
                    ], |row| {{
                        row.get(0)
                    }})?;

                    let count: usize = rows.next().unwrap()?;
                    Ok(count)
                }}

                pub fn last_{lower_case}_for_system(
                    &self,
                    system_and_component_ids: (u8, u8)
                ) -> Result<Option<mavspec::rust::dialects::common::messages::{type_name}>, DbError> {{
                    let conn = self.conn();
                    conn.busy_timeout(std::time::Duration::from_millis(10))?;

                    let system_id = system_and_component_ids.0;
                    let component_id = system_and_component_ids.1;

                    let mut stmt = conn.prepare(\"
                        SELECT {fields} FROM messages_{lower_case}
                        WHERE system_id=:system_id AND component_id=:component_id
                        ORDER BY received_at DESC
                        LIMIT 1\")?;

                    let mut rows = stmt.query_map(&[
                        (\":system_id\", &system_id),
                        (\":component_id\", &component_id)
                    ], |row| {{
                        Ok(mavspec::rust::dialects::common::messages::{type_name} {{
                            {values}
                        }})
                    }})?;

                    let row = rows.next().transpose()?;
                    Ok(row)
                }}
                "
            )
        })
        .collect();

    let match_arms_count: String = dialect
        .messages()
        .into_iter()
        .map(|m| {
            let lower_case = m.name().to_lowercase();
            let type_name: String = lower_case
                .split("_")
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            format!(
                "\"{}\" => self.count_{lower_case}_for_system(system_and_component_ids)?,",
                m.name()
            )
        })
        .collect();

    let match_arms_last: String = dialect
        .messages()
        .into_iter()
        .map(|m| {
            let lower_case = m.name().to_lowercase();
            let type_name: String = lower_case
                .split("_")
                .map(|s| {
                    let (h, t) = s.split_at(1);
                    format!("{}{}", h.to_uppercase(), t)
                })
                .collect();

            format!("\"{}\" => self.last_{lower_case}_for_system(system_and_component_ids)?.map(|inner| mavspec::rust::dialects::Common::{type_name}(inner)),", m.name())
        })
        .collect();

    methods.extend(
        format!(
            "
            pub fn common_timeseries_by_name_for_system(
                &self,
                msg_name: &str,
                field_name: &str,
                system_and_component_ids: (u8, u8)
            ) -> Result<Vec<(chrono::DateTime<chrono::Utc>, f64)>, DbError> {{
                use chrono::TimeZone;

                let conn = self.conn();
                let system_id = system_and_component_ids.0;
                let component_id = system_and_component_ids.1;
                let lower_case = msg_name.to_lowercase();

                // Look Ma, SQL injection
                let query = format!(\"SELECT received_at, {{field_name}} FROM messages_{{lower_case}}
                    WHERE system_id=:system_id AND component_id=:component_id
                    ORDER BY received_at ASC\");

                let mut stmt = conn.prepare(&query)?;
                let mut rows = stmt.query_map(&[
                    (\":system_id\", &system_id),
                    (\":component_id\", &component_id)
                ], |row| {{
                    let timestamp: chrono::DateTime<chrono::Utc> = row.get(0)?;
                    let value: f64 = row.get(1)?;
                    Ok((timestamp, value))
                }})?;

                let timeseries = rows.collect::<Result<Vec<(chrono::DateTime<chrono::Utc>, f64)>, _>>()?;

                Ok(timeseries)
            }}

            pub fn common_count_by_name_for_system(
                &self,
                msg_name: &str,
                system_and_component_ids: (u8, u8)
            ) -> Result<usize, DbError> {{
                use mavspec::rust::dialects::Common;
                let result = match msg_name {{
                    {match_arms_count}
                    _ => 0
                }};
                Ok(result)
            }}

            pub fn last_common_message_by_name_for_system(
                &self,
                msg_name: &str,
                system_and_component_ids: (u8, u8)
            ) -> Result<Option<mavspec::rust::dialects::Common>, DbError> {{
                use mavspec::rust::dialects::Common;
                let result = match msg_name {{
                    {match_arms_last}
                    _ => None
                }};
                Ok(result)
            }}
            "
        )
        .chars(),
    );

    let output = format!("impl Db {{\n{methods}\n}}");
    output.parse().unwrap()
}

struct QuantityInformation {}

fn generate_quantity_information(
    field: &mavinspect::protocol::MessageField,
) -> proc_macro2::TokenStream {
    let value_type = match field.r#type() {
        MavType::UInt8 => quote::format_ident!("u8"),
        MavType::UInt16 => quote::format_ident!("u16"),
        MavType::UInt32 => quote::format_ident!("u32"),
        MavType::UInt64 => quote::format_ident!("u64"),
        MavType::Int8 => quote::format_ident!("i8"),
        MavType::Int16 => quote::format_ident!("i16"),
        MavType::Int32 => quote::format_ident!("i32"),
        MavType::Int64 => quote::format_ident!("i64"),
        MavType::Float => quote::format_ident!("f32"),
        MavType::Double => quote::format_ident!("f64"),
        MavType::Char => quote::format_ident!("char"),
        MavType::UInt8MavlinkVersion => quote::format_ident!("u8"), //TODO Hans: How to handle this?
        MavType::Array(_mav_type, _size) => quote::format_ident!("u8"), //TODO Hans: Handle this
    };
    return quote::quote!(crate::metrics::Unquantified<#value_type>);
}

#[proc_macro]
pub fn metric(input: TokenStream) -> TokenStream {
    //let ty = parse_macro_input!(input as Type);

    // Generate the fully qualified trait syntax
    let expanded = quote::quote! {
         <crate::metrics::heartbeat::Message as metrics::Heartbeat>::Type
    };

    TokenStream::from(expanded)
}

#[proc_macro]
pub fn generate_metrics(_item: TokenStream) -> TokenStream {
    let protocol = mavspec::definitions::protocol();
    let dialect = protocol.get_dialect_by_name("common").unwrap();

    dialect
        .messages()
        .into_iter()
        .map(|msg| {
            let msg_trait = quote::format_ident!(
                "{}",
                convert_case::Casing::to_case(&msg.name().to_string(), convert_case::Case::Pascal)
            );
            let msg_mod = quote::format_ident!(
                "{}",
                convert_case::Casing::to_case(&msg.name().to_string(), convert_case::Case::Snake)
            );
            let msg_members = msg
                .fields()
                .iter()
                .map(|f| {
                    quote::format_ident!(
                        "{}",
                        convert_case::Casing::to_case(
                            &format!("{}", f.name()),
                            convert_case::Case::Pascal,
                        )
                    )
                })
                .collect::<Vec<_>>();

            let quantities = msg
                .fields()
                .iter()
                .map(|f| generate_quantity_information(f))
                .collect::<Vec<_>>();

            let names = msg_members
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();

            quote::quote!(

                pub trait #msg_trait : Message {
                    #(type #msg_members /* : FieldOf<Self>*/;)*
                }

                pub mod #msg_mod {
                    pub struct Message {}
                    impl crate::metrics::sealed::Sealed for Message {}
                    impl crate::metrics::Message for Message {}
                    impl crate::metrics::#msg_trait for Message {
                        #(type #msg_members = fields::#msg_members;)*
                    }

                    mod fields {
                        #(pub struct #msg_members {})*

                        //#(impl FieldOf<#msg_struct> for #field_structs {})*

                        #(impl crate::metrics::sealed::Sealed for #msg_members {})*

                        #(impl crate::metrics::Metric for #msg_members {
                            type Quantity = #quantities;
                            fn name() -> &'static str {
                                return #names;
                            }
                        })*
                    }
                }
            )
        })
        .collect::<proc_macro2::TokenStream>()
        .into()
}
