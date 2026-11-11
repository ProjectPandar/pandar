use std::collections::BTreeMap;

use anyhow::Context;
use pandar_protocol::agent::v1::PrintProjectFile;

use super::{MoonrakerClient, projection::UPLOAD_ROOT};
use crate::machine::{MachineJsonPayload, PrintProjectDispatchResult, brtc::md5_upper};

pub(super) async fn print_gcode(
    client: &MoonrakerClient,
    command: &PrintProjectFile,
    artifact: Vec<u8>,
) -> anyhow::Result<PrintProjectDispatchResult> {
    let md5 = md5_upper(&artifact);
    let directory = format!("{UPLOAD_ROOT}/{}", command.job_id);
    let filename = command
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&command.filename);
    let uploaded = client
        .upload_gcode(&directory, filename, artifact)
        .await
        .with_context(|| format!("upload {filename} to Moonraker"))?;
    let path = uploaded.item.path;
    client
        .start_print(&path)
        .await
        .with_context(|| format!("start Moonraker print of {path}"))?;

    Ok(PrintProjectDispatchResult {
        topic: "moonraker/printer/print/start".to_owned(),
        payload: MachineJsonPayload::Object(BTreeMap::from([(
            "filename".to_owned(),
            MachineJsonPayload::String(path.clone()),
        )])),
        qos: 0,
        uploaded_url: client.resolve_url(&format!("/server/files/gcodes/{path}")),
        uploaded_path: path,
        md5,
    })
}
