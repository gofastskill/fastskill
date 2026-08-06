//! Command registration that keeps output out of the JSON-RPC stream.
//!
//! `AppBuilder::register` requires the handler's future to be `'static`, so a
//! handler can read the [`AppContext`] synchronously but cannot write back to it
//! once it starts awaiting. That is precisely what routing output to
//! `ctx.framework_println` needs, and it is why `fastskill mcp serve` returned
//! the literal string `"OK"` for every tool while the real output went to the
//! process's `stdout`.
//!
//! `Command::execute` itself is more permissive than the `register` helper: its
//! future may borrow the context (`Send + 'a`). [`AppBuilderExt::register_out`]
//! builds that `Command` directly, keeping the existing handler shape while
//! wrapping the returned future so buffered output is drained into the context
//! after it resolves.

use cli_framework::app::context::AppContext;
use cli_framework::command::{Command, TypedArgs};
use cli_framework::prelude::AppBuilder;
use cli_framework::spec::command_tree::CommandPath;
use std::future::Future;
use std::sync::Arc;

use crate::output::{self, Mode};

/// Registration helpers for commands whose output must survive MCP dispatch.
pub trait AppBuilderExt: Sized {
    /// Register a typed command, routing its [`crate::outln!`] output through
    /// the context under [`Mode::Capture`].
    ///
    /// The handler keeps the same shape as `AppBuilder::register`: it may read
    /// the context synchronously and returns a `'static` future.
    fn register_out<T, F, Fut>(self, path: CommandPath, handler: F) -> anyhow::Result<Self>
    where
        T: TypedArgs,
        F: Fn(&mut dyn AppContext, T) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static;

    /// As [`register_out`](AppBuilderExt::register_out), but the command is not
    /// exported as an MCP tool.
    ///
    /// For commands that never return (`serve`) or that are meaningless over a
    /// request/response tool call.
    fn register_out_no_mcp<T, F, Fut>(self, path: CommandPath, handler: F) -> anyhow::Result<Self>
    where
        T: TypedArgs,
        F: Fn(&mut dyn AppContext, T) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static;
}

/// Build a `Command` whose execute future drains captured output into the
/// context before returning.
fn build_command<T, F, Fut>(path: &CommandPath, handler: F, expose_mcp: bool) -> Command
where
    T: TypedArgs,
    F: Fn(&mut dyn AppContext, T) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let spec = Arc::new(T::command_spec());
    let id: Arc<str> = Arc::from(path.leaf().unwrap_or(""));
    let handler = Arc::new(handler);

    Command {
        id,
        spec,
        validator: None,
        expose_mcp,
        expose_chat: expose_mcp,
        meta: None,
        visibility: None,
        execute: Arc::new(move |ctx, args| {
            let typed = T::from_arg_value_map(&args);
            // The handler reads `ctx` synchronously; the future it returns is
            // `'static` and does not hold the borrow, which frees `ctx` for the
            // drain below.
            let fut = handler(ctx, typed);
            Box::pin(async move {
                match output::mode() {
                    Mode::Direct => fut.await,
                    Mode::Capture => {
                        let (result, text) = output::capture(fut).await;
                        // Drain even on failure: partial output is often the
                        // most useful part of a failed tool call.
                        for line in text.lines() {
                            ctx.framework_println(line);
                        }
                        result
                    }
                }
            })
        }),
    }
}

impl AppBuilderExt for AppBuilder {
    fn register_out<T, F, Fut>(self, path: CommandPath, handler: F) -> anyhow::Result<Self>
    where
        T: TypedArgs,
        F: Fn(&mut dyn AppContext, T) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let command = build_command::<T, F, Fut>(&path, handler, true);
        self.register_command_at(&path, command)
    }

    fn register_out_no_mcp<T, F, Fut>(self, path: CommandPath, handler: F) -> anyhow::Result<Self>
    where
        T: TypedArgs,
        F: Fn(&mut dyn AppContext, T) -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let command = build_command::<T, F, Fut>(&path, handler, false);
        self.register_command_at(&path, command)
    }
}
