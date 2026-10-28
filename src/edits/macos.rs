use std::ptr::NonNull;

use anyhow::{Context, Result};
use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFRetained, CFString, CFType};

/// The focused text box, read through its accessibility value.
pub struct Field(CFRetained<AXUIElement>);

/// The focused text box; none for password fields.
pub fn focused() -> Result<Vec<Field>> {
    let system = unsafe { AXUIElement::new_system_wide() };
    let element = attribute(&system, "AXFocusedUIElement")
        .and_then(|v| v.downcast::<AXUIElement>().ok())
        .context("no focused element")?;
    unsafe { element.set_messaging_timeout(1.0) }; // a hung app shouldn't stall the watch
    let secure = ["AXRole", "AXSubrole"].iter().any(|name| {
        attribute(&element, name)
            .and_then(|v| v.downcast::<CFString>().ok())
            .is_some_and(|v| v.to_string() == "AXSecureTextField")
    });
    Ok(if secure { Vec::new() } else { vec![Field(element)] })
}

impl Field {
    pub fn read(&self) -> Option<String> {
        let text = attribute(&self.0, "AXValue")?.downcast::<CFString>().ok()?.to_string();
        (text.len() <= super::MAX_TEXT).then_some(text)
    }
}

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    let status = unsafe { element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value)) };
    if status != AXError::Success {
        return None;
    }
    Some(unsafe { CFRetained::from_raw(NonNull::new(value.cast_mut())?) })
}
