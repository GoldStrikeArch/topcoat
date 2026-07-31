//! One [`WriteDom`](crate::WriteDom) implementation per upstream AST file.
//!
//! The modules hold nothing but trait implementations and the helpers they need;
//! they mirror the layout of `topcoat-view-grammar` so a variant added upstream
//! lands in the file that matches it.

mod attribute;
mod attributes;
mod bind_attribute;
mod component;
mod control_flow;
mod element;
mod event_handler;
mod expr;
mod local;
mod node;
mod nodes;
mod signal_declaration;

pub(crate) use component::*;
pub(crate) use control_flow::*;
