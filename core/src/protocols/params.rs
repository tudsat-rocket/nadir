use core::f32;
use std::{collections::HashMap, time::Duration};

use maviola::protocol::ComponentId;
use mavspec::rust::dialects::{
    Common,
    common::{
        enums::{MavParamType, MavResult},
        messages::ParamRequestList,
    },
};
use tokio::time::timeout;

use crate::System;

pub type ParamId = String;

#[derive(Debug)]
pub struct Param {
    pub id: ParamId,
    pub value: f32,
    pub downloaded_value: f32,
    pub param_type: MavParamType,
}

// TODO: rename
pub enum ParamProgress {
    Unknown,
    Failed(MavResult),
    Progress(usize, usize),
    Complete(HashMap<ParamId, Param>),
}

pub async fn download_params(
    system: System,
    component_id: ComponentId,
    mut message_rx: tokio::sync::broadcast::Receiver<Common>,
) {
    // Wait for the first AUTOPILOT_VERSION message. We need the device capabilities to check for
    // the flags which tell us how the parameter values are encoded.
    //let capabilities = loop {
    //    if let Ok(Common::AutopilotVersion(av)) = message_rx.recv().await {
    //        break av.capabilities;
    //    }
    //};

    tracing::debug!(system_id = system.system_id, "Downloading params");

    // First we download a complete list of parameters...
    // TODO: more robust behaviour for failures, manually redownload missed params, etc.
    system.send_message(&ParamRequestList {
        target_system: system.system_id,
        target_component: component_id,
    });

    let mut number_params: Option<usize> = None;
    let mut params: HashMap<ParamId, Param> = HashMap::new();
    while number_params.is_none_or(|num| params.len() < num) {
        match timeout(Duration::from_millis(1000), async {
            loop {
                if let Ok(Common::ParamValue(value)) = message_rx.recv().await {
                    return value;
                }
            }
        })
        .await
        {
            Ok(param) => {
                let id = String::from_utf8_lossy(&param.param_id).to_string();
                let count = param.param_count as usize;
                number_params = Some(count);
                params.insert(
                    id.trim_matches('\0').to_string(),
                    Param {
                        id,
                        param_type: param.param_type,
                        value: param.param_value,
                        downloaded_value: param.param_value,
                    },
                );

                *system.params.lock().unwrap() =
                    ParamProgress::Progress(param.param_index as usize, count);
            }
            Err(e) => {
                // TODO: back off here, or check MAVLink capabilities first?
                tracing::error!("Parameter download failed ({e:?}), retrying in 5s.");
                tokio::time::sleep(Duration::from_millis(5000)).await;
                break;
            }
        }
    }

    *system.params.lock().unwrap() = ParamProgress::Complete(params);

    // Now we maintain the list. If a change is made from the UI, the system responds with another
    // PARAM_VALUE message, so we listen for these from this task and keep our parameter storage
    // updated with the downloaded (saved to the system/vehicle) values.
    loop {
        if let Ok(Common::ParamValue(value)) = message_rx.recv().await {
            let id = String::from_utf8_lossy(&value.param_id)
                .trim_matches('\0')
                .to_string();
            let mut progress = system.params.lock().unwrap();
            let ParamProgress::Complete(params) = &mut *progress else {
                continue;
            };

            if let Some(param) = params.get_mut(&id) {
                param.downloaded_value = value.param_value;
            }
        }
    }
}
