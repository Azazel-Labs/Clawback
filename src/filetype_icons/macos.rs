use eframe::egui::ColorImage;
use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
use objc2_foundation::{NSDictionary, NSString};

pub fn load(extension: Option<&str>, size: u32) -> Option<ColorImage> {
    objc2::rc::autoreleasepool(|_| {
        let extension = extension.map_or("", |extension| extension.trim_start_matches('.'));
        // This API also handles unknown extensions using Finder's generic file icon.
        #[allow(deprecated)]
        let icon = NSWorkspace::sharedWorkspace().iconForFileType(&NSString::from_str(extension));
        let tiff = icon.TIFFRepresentation()?;
        let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)?;
        // SAFETY: An empty dictionary has no values with an incorrect property type.
        let png =
            unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
        super::decode(&png.to_vec(), size)
    })
}
