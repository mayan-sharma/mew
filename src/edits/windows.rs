use anyhow::Result;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationValuePattern,
    UIA_TextPatternId, UIA_ValuePatternId,
};

#[derive(Clone, Copy)]
enum Source {
    Value,
    Text,
}

/// One way to read the focused element's text.
pub struct Field {
    _uia: IUIAutomation,
    element: IUIAutomationElement,
    source: Source,
}

/// The focused element's possible readings; none for password fields.
pub fn focused() -> Result<Vec<Field>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let element = uia.GetFocusedElement()?;
        if element.CurrentIsPassword()?.as_bool() {
            return Ok(Vec::new());
        }
        Ok([Source::Value, Source::Text]
            .map(|source| Field { _uia: uia.clone(), element: element.clone(), source })
            .into())
    }
}

impl Field {
    pub fn read(&self) -> Option<String> {
        unsafe {
            Some(match self.source {
                Source::Value => self
                    .element
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                    .ok()?
                    .CurrentValue()
                    .ok()?
                    .to_string(),
                Source::Text => self
                    .element
                    .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                    .ok()?
                    .DocumentRange()
                    .ok()?
                    .GetText(super::MAX_TEXT as i32)
                    .ok()?
                    .to_string(),
            })
        }
    }
}
