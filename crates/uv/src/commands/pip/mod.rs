use std::borrow::Cow;

use uv_configuration::TargetTriple;
use uv_platform_tags::{Tags, TagsError};
use uv_pypi_types::ResolverMarkerEnvironment;
use uv_python::{Interpreter, PythonVersion};

pub mod check;
pub mod compile;
pub mod freeze;
pub mod install;
pub mod latest;
pub mod list;
pub mod loggers;
pub mod operations;
pub mod show;
pub mod sync;
pub mod tree;
pub mod uninstall;

pub fn resolution_markers(
    python_version: Option<&PythonVersion>,
    python_platform: Option<&TargetTriple>,
    interpreter: &Interpreter,
) -> ResolverMarkerEnvironment {
    match (python_platform, python_version) {
        (Some(python_platform), Some(python_version)) => ResolverMarkerEnvironment::from(
            python_version.markers(&python_platform.markers(interpreter.markers())),
        ),
        (Some(python_platform), None) => {
            ResolverMarkerEnvironment::from(python_platform.markers(interpreter.markers()))
        }
        (None, Some(python_version)) => {
            ResolverMarkerEnvironment::from(python_version.markers(interpreter.markers()))
        }
        (None, None) => interpreter.resolver_marker_environment(),
    }
}

pub fn resolution_tags<'env>(
    python_version: Option<&PythonVersion>,
    python_platform: Option<&TargetTriple>,
    interpreter: &'env Interpreter,
) -> Result<Cow<'env, Tags>, TagsError> {
    Ok(match (python_platform, python_version.as_ref()) {
        (Some(python_platform), Some(python_version)) => Cow::Owned(Tags::from_env(
            &python_platform.platform(),
            (python_version.major(), python_version.minor()),
            interpreter.implementation_name(),
            interpreter.implementation_tuple(),
            python_platform.manylinux_compatible(),
            interpreter.gil_disabled(),
        )?),
        (Some(python_platform), None) => Cow::Owned(Tags::from_env(
            &python_platform.platform(),
            interpreter.python_tuple(),
            interpreter.implementation_name(),
            interpreter.implementation_tuple(),
            python_platform.manylinux_compatible(),
            interpreter.gil_disabled(),
        )?),
        (None, Some(python_version)) => Cow::Owned(Tags::from_env(
            interpreter.platform(),
            (python_version.major(), python_version.minor()),
            interpreter.implementation_name(),
            interpreter.implementation_tuple(),
            interpreter.manylinux_compatible(),
            interpreter.gil_disabled(),
        )?),
        (None, None) => Cow::Borrowed(interpreter.tags()?),
    })
}

/// Determine the tags, markers, and interpreter to use for resolution.
pub fn resolution_environment(
    python_version: Option<PythonVersion>,
    python_platform: Option<TargetTriple>,
    interpreter: &Interpreter,
) -> Result<(Cow<'_, Tags>, ResolverMarkerEnvironment), TagsError> {
    let tags = match (python_platform, python_version.as_ref()) {
        (Some(python_platform), Some(python_version)) => Cow::Owned(Tags::from_env(
            &python_platform.platform(),
            (python_version.major(), python_version.minor()),
            interpreter.implementation_name(),
            interpreter.implementation_tuple(),
            python_platform.manylinux_compatible(),
            interpreter.gil_disabled(),
        )?),
        (Some(python_platform), None) => Cow::Owned(Tags::from_env(
            &python_platform.platform(),
            interpreter.python_tuple(),
            interpreter.implementation_name(),
            interpreter.implementation_tuple(),
            python_platform.manylinux_compatible(),
            interpreter.gil_disabled(),
        )?),
        (None, Some(python_version)) => Cow::Owned(Tags::from_env(
            interpreter.platform(),
            (python_version.major(), python_version.minor()),
            interpreter.implementation_name(),
            interpreter.implementation_tuple(),
            interpreter.manylinux_compatible(),
            interpreter.gil_disabled(),
        )?),
        (None, None) => Cow::Borrowed(interpreter.tags()?),
    };

    // Apply the platform tags to the markers.
    let markers = match (python_platform, python_version) {
        (Some(python_platform), Some(python_version)) => ResolverMarkerEnvironment::from(
            python_version.markers(&python_platform.markers(interpreter.markers())),
        ),
        (Some(python_platform), None) => {
            ResolverMarkerEnvironment::from(python_platform.markers(interpreter.markers()))
        }
        (None, Some(python_version)) => {
            ResolverMarkerEnvironment::from(python_version.markers(interpreter.markers()))
        }
        (None, None) => interpreter.resolver_marker_environment(),
    };

    Ok((tags, markers))
}
