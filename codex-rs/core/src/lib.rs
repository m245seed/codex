//! Root of the `codex-core` library.

#![deny(clippy::print_stdout, clippy::print_stderr)]

// Private modules
mod apply_patch;
mod chat_completions;
mod client;
mod client_common;
mod codex_conversation;
mod conversation_history;
mod environment_context;
mod exec_command;
mod flags;
mod is_safe_command;
mod mcp_connection_manager;
mod mcp_tool_call;
mod message_history;
mod model_provider_info;
mod truncate;
mod unified_exec;
mod user_instructions;
mod conversation_manager;
mod event_mapping;
mod openai_model_info;
mod openai_tools;
mod rollout;
mod tool_apply_patch;
mod user_notification;

// Public modules
pub mod auth;
pub mod bash;
pub mod codex;
pub mod config;
pub mod config_edit;
pub mod config_profile;
pub mod config_types;
pub mod custom_prompts;
pub mod default_client;
pub mod error;
pub mod exec;
pub mod exec_env;
pub mod git_info;
pub mod internal_storage;
pub mod landlock;
pub mod model_family;
pub mod parse_command;
pub mod plan_tool;
pub mod project_doc;
pub mod review_format;
pub mod seatbelt;
pub mod shell;
pub mod spawn;
pub mod terminal;
pub mod token_data;
pub mod turn_diff_tracker;
pub mod util;
pub(crate) mod safety;

// Re-exports
pub use codex_conversation::CodexConversation;
pub use model_provider_info::{BUILT_IN_OSS_MODEL_PROVIDER_ID, ModelProviderInfo, WireApi, built_in_model_providers, create_oss_provider_with_base_url};
pub use conversation_manager::{ConversationManager, NewConversation};
pub use auth::{AuthManager, CodexAuth};
pub use rollout::{ARCHIVED_SESSIONS_SUBDIR, RolloutRecorder, SESSIONS_SUBDIR, SessionMeta, find_conversation_path_by_id_str};
pub use rollout::list::{ConversationItem, ConversationsPage, Cursor};
pub use apply_patch::CODEX_APPLY_PATCH_ARG1;
pub use safety::get_platform_sandbox;
pub use codex_protocol::{protocol, config_types as protocol_config_types};
pub use codex_protocol::protocol::InitialHistory;
pub use client::ModelClient;
pub use client_common::{Prompt, REVIEW_PROMPT, ResponseEvent, ResponseStream};
pub use codex::compact::{content_items_to_text, is_session_prefix_message};
pub use codex_protocol::models::{ContentItem, LocalShellAction, LocalShellExecAction, LocalShellStatus, ReasoningItemContent, ResponseItem};
