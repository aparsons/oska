// SPDX-License-Identifier: Apache-2.0
//! Where what a command reports is: how a diagnostic names its file.

use crate::package;
use oska_syntax::{Diagnostic, Frame, Span};

/// How a command's output names the place of what it reports: the
/// file's path and text for a single file; for a package, its
/// [`oska_check::Locator`], and the package directory as given, which
/// the human output's paths start from (JSON keeps them package-relative).
pub(crate) enum Places<'p> {
    /// What is reported is located already, or has no place.
    Located,
    File {
        path: &'p str,
        src: &'p str,
    },
    Package {
        locator: oska_check::Locator<'p>,
        dir: &'p str,
        json: bool,
    },
}

impl Places<'_> {
    /// `diagnostic`, whose span is an offset into the program, named by
    /// its file and line.
    pub(crate) fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        match self {
            Places::Located => diagnostic,
            Places::File { path, src } => match diagnostic.location {
                Some(_) => diagnostic,
                None => diagnostic.locate(path, src),
            },
            Places::Package { locator, dir, json } => {
                let located = locator.locate(diagnostic);
                if *json {
                    located
                } else {
                    from_here(dir, located)
                }
            }
        }
    }

    /// `frame` of a panic's call stack, whose span is an offset into the
    /// program, placed as [`Places::locate`] places a diagnostic's frames.
    pub(crate) fn locate_frame(&self, frame: Frame) -> Frame {
        match self {
            Places::Located => frame,
            Places::File { path, src } => frame.locate(path, src),
            Places::Package { locator, dir, json } => {
                let mut located = locator.locate_frame(frame);
                if !*json {
                    frame_from_here(dir, &mut located);
                }
                located
            }
        }
    }

    /// The module of the test whose item is at `span` in the program.
    pub(crate) fn module_of(&self, span: Span) -> Option<String> {
        match self {
            Places::Located | Places::File { .. } => None,
            Places::Package { locator, .. } => locator.module_of(span).map(str::to_string),
        }
    }
}

/// `diagnostic`, located in a package in `dir`, with its path written
/// from the current directory.
pub(crate) fn from_here(dir: &str, mut diagnostic: Diagnostic) -> Diagnostic {
    if let Some(location) = &mut diagnostic.location {
        location.path = package::from_here(dir, &location.path);
    }
    if let Some(frames) = &mut diagnostic.frames {
        for frame in &mut frames.frames {
            frame_from_here(dir, frame);
        }
    }
    diagnostic
}

/// `frame`, located in a package in `dir`, with its path written from
/// the current directory.
pub(crate) fn frame_from_here(dir: &str, frame: &mut Frame) {
    if let Some(location) = &mut frame.location {
        location.path = package::from_here(dir, &location.path);
    }
}

/// A native executable's panic reports name places as `oskac` reports
/// them: placed by [`Places`].
impl oska_native::Locate for Places<'_> {
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        Places::locate(self, diagnostic)
    }

    fn locate_frame(&self, frame: Frame) -> Frame {
        Places::locate_frame(self, frame)
    }

    fn places(&self) -> bool {
        !matches!(self, Places::Located)
    }
}

/// `report`, of the package in `dir`, with every path written from the
/// current directory, for the human output.
pub(crate) fn report_from_here(
    dir: &str,
    mut report: oska_check::CheckReport,
) -> oska_check::CheckReport {
    report.diagnostics = report
        .diagnostics
        .into_iter()
        .map(|d| from_here(dir, d))
        .collect();
    for site in &mut report.audit {
        if let Some(location) = &mut site.location {
            location.path = package::from_here(dir, &location.path);
        }
    }
    for attenuation in &mut report.attenuations {
        if let Some(location) = &mut attenuation.location {
            location.path = package::from_here(dir, &location.path);
        }
    }
    report
}
