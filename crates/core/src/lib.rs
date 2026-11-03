//! # materialcraft-core
//!
//! The MaterialCraft document model, L0 (pure data, no I/O): a [`Package`] holds [`Graph`]s and
//! [`Resource`]s; a graph holds [`Node`]s (catalog definitions with parameter overrides),
//! [`Connection`]s between named ports, frames, comments, navigation pins and exposed inputs.
//! Parameter values are [`Value`]s; function graphs ([`function::FnGraph`]) drive pixel and value
//! processors.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod function;
pub mod graph;
pub mod package;
pub mod value;

pub use graph::{
    Attributes, BaseParams, Comment, Connection, ExposedParam, Frame, Graph, GraphKind, GraphTopology, MAX_SIZE_LOG2, MIN_SIZE_LOG2, Node, NodeId,
    OutputFormat, Pin, Preset, SizeMode,
};
pub use package::{PACKAGE_EXT, Package, Resource};
pub use value::{Curve, CurvePoint, Gradient, GradientKey, Value, linear_to_srgb, parse_color, srgb_to_linear};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("no node {0}")]
    NoNode(u64),
    #[error("no graph {0}")]
    NoGraph(usize),
    #[error("that connection would make a cycle")]
    Cycle,
}

pub type Result<T> = std::result::Result<T, CoreError>;
