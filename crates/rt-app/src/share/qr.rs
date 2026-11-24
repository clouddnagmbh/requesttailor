// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The QR code of a link, as SVG text (no image library).

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

use super::{MAX_QR_LEN, ShareError};

/// The SVG of a QR code for `text`. Rejects text longer than [`MAX_QR_LEN`] with the hint the
/// app shows.
pub fn qr_svg(text: &str) -> Result<String, ShareError> {
    if text.chars().count() > MAX_QR_LEN {
        return Err(ShareError::Unavailable(
            "too long for a QR code, copy the link or save a file".into(),
        ));
    }
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L).map_err(|_| {
        ShareError::Unavailable("too long for a QR code, copy the link or save a file".into())
    })?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}
