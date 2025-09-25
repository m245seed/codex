use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::codex::Session;
use crate::openai_tools::{JsonSchema, OpenAiTool, ResponsesApiTool};
use crate::protocol::{Event, EventMsg};
use codex_protocol::models::{FunctionCallOutputPayload, ResponseInputItem};

pub use codex_protocol::plan_tool::{PlanItemArg, StepStatus, UpdatePlanArgs};

const PLAN_UPDATED: &str = "Plan updated";
const PARSE_ERROR_PREFIX: &str = "failed to parse function arguments: ";

pub(crate) static PLAN_TOOL: LazyLock<OpenAiTool> = LazyLock::new(|| {
    OpenAiTool::Function(ResponsesApiTool {
        name: "update_plan".into(),
        description: "Updates the task plan.\nProvide an optional explanation and a list of plan items, each with a step and status.\nAt most one step can be in_progress at a time.".into(),
        strict: false,
        parameters: JsonSchema::Object {
            properties: BTreeMap::from([
                ("explanation".into(), JsonSchema::String { description: None }),
                ("plan".into(), JsonSchema::Array {
                    description: Some("The list of steps".into()),
                    items: Box::new(JsonSchema::Object {
                        properties: BTreeMap::from([
                            ("step".into(), JsonSchema::String { description: None }),
                            ("status".into(), JsonSchema::String {
                                description: Some("One of: pending, in_progress, completed".into()),
                            }),
                        ]),
                        required: Some(vec!["step".into(), "status".into()]),
                        additional_properties: Some(false),
                    }),
                }),
            ]),
            required: Some(vec!["plan".into()]),
            additional_properties: Some(false),
        },
    })
});

pub(crate) async fn handle_update_plan(
    session: &Session,
    arguments: String,
    sub_id: String,
    call_id: String,
) -> ResponseInputItem {
    match serde_json::from_str::<UpdatePlanArgs>(&arguments) {
        Ok(args) => {
            session.send_event(Event {
                id: sub_id,
                msg: EventMsg::PlanUpdate(args),
            }).await;
            
            ResponseInputItem::FunctionCallOutput {
                call_id,
                output: FunctionCallOutputPayload {
                    content: PLAN_UPDATED.into(),
                    success: Some(true),
                },
            }
        }
        Err(e) => ResponseInputItem::FunctionCallOutput {
            call_id,
            output: FunctionCallOutputPayload {
                content: format!("{PARSE_ERROR_PREFIX}{e}"),
                success: None,
            },
        },
    }
}


