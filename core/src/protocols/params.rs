use core::f32;
use std::collections::HashMap;

use maviola::protocol::ComponentId;
use mavspec::rust::{
    default_dialect::messages::ParamValue,
    dialects::{
        Common,
        common::{
            enums::{MavParamType, MavResult},
            messages::{ParamRequestList, ParamRequestRead},
        },
    },
};

use crate::{
    System,
    protocols::{Gatherable, gather},
};

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

impl Gatherable for ParamValue {
    type InitialRequest = ParamRequestList;
    type SpecificRequest = ParamRequestRead;

    fn index(&self) -> usize {
        self.param_index as usize
    }

    fn count(&self) -> usize {
        self.param_count as usize
    }

    fn unpack(msg: Common) -> Option<Self> {
        match msg {
            Common::ParamValue(inner) => Some(inner),
            _ => None,
        }
    }

    fn initial_request(system_id: u8, component_id: u8) -> Self::InitialRequest {
        ParamRequestList {
            target_system: system_id,
            target_component: component_id,
        }
    }

    fn specific_request(system_id: u8, component_id: u8, index: usize) -> Self::SpecificRequest {
        ParamRequestRead {
            target_system: system_id,
            target_component: component_id,
            param_id: [0x00; 16],
            param_index: index as i16,
        }
    }
}

impl From<ParamValue> for Param {
    fn from(value: ParamValue) -> Self {
        let id = String::from_utf8_lossy(&value.param_id).to_string();
        let trimmed = id.trim_matches('\0').to_string();

        Param {
            id: trimmed,
            param_type: value.param_type,
            value: value.param_value,
            downloaded_value: value.param_value,
        }
    }
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

    let params = system.params.clone();
    let result = gather(
        &system,
        component_id,
        &mut message_rx,
        Some(Box::new(move |received, total| {
            *params.lock().unwrap() = ParamProgress::Progress(received, total);
        })),
    )
    .await;

    match result {
        Ok(params_vec) => {
            let map: HashMap<_, _> = params_vec
                .into_iter()
                .map(|p: ParamValue| {
                    let param: Param = p.into();
                    (param.id.clone(), param)
                })
                .collect();

            *system.params.lock().unwrap() = ParamProgress::Complete(map);
        }
        Err(res) => *system.params.lock().unwrap() = ParamProgress::Failed(res),
    }

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
