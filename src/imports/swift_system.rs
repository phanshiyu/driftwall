//! The Swift modules a toolchain or an Apple SDK ships, as a closed list.
//!
//! An `import` of one of these is a `stdlib` package use, anything else a `third-party` one. See
//! "Package uses" in `docs/DESIGN.md`, which lists the same names.

/// Every system module, sorted.
const NAMES: &[&str] = &[
    "AVFAudio",
    "AVFoundation",
    "AVKit",
    "Accelerate",
    "Accessibility",
    "ActivityKit",
    "AppIntents",
    "AppKit",
    "ApplicationServices",
    "AudioToolbox",
    "AudioUnit",
    "AuthenticationServices",
    "BackgroundTasks",
    "CFNetwork",
    "CallKit",
    "Carbon",
    "Charts",
    "Cocoa",
    "Combine",
    "CompilerPluginSupport",
    "Contacts",
    "CoreAudio",
    "CoreAudioKit",
    "CoreBluetooth",
    "CoreData",
    "CoreFoundation",
    "CoreGraphics",
    "CoreHaptics",
    "CoreImage",
    "CoreLocation",
    "CoreMIDI",
    "CoreML",
    "CoreMedia",
    "CoreMotion",
    "CoreServices",
    "CoreSpotlight",
    "CoreText",
    "CoreTransferable",
    "CoreVideo",
    "CryptoKit",
    "Darwin",
    "DeviceCheck",
    "Dispatch",
    "Distributed",
    "EventKit",
    "ExternalAccessory",
    "Foundation",
    "FoundationEssentials",
    "FoundationNetworking",
    "FoundationXML",
    "GLKit",
    "GameController",
    "GameKit",
    "GameplayKit",
    "Glibc",
    "GroupActivities",
    "HealthKit",
    "IOKit",
    "IOSurface",
    "ImageIO",
    "Intents",
    "LinkPresentation",
    "LocalAuthentication",
    "MapKit",
    "MediaPlayer",
    "MessageUI",
    "Messages",
    "Metal",
    "MetalKit",
    "MetalPerformanceShaders",
    "ModelIO",
    "MultipeerConnectivity",
    "NaturalLanguage",
    "Network",
    "NetworkExtension",
    "NotificationCenter",
    "OSLog",
    "ObjectiveC",
    "Observation",
    "OpenGL",
    "PDFKit",
    "PackageDescription",
    "PassKit",
    "PencilKit",
    "Photos",
    "PhotosUI",
    "PushKit",
    "QuartzCore",
    "QuickLook",
    "RealityKit",
    "RegexBuilder",
    "ReplayKit",
    "SafariServices",
    "SceneKit",
    "ScreenCaptureKit",
    "Security",
    "ServiceManagement",
    "Speech",
    "SpriteKit",
    "StoreKit",
    "Swift",
    "SwiftData",
    "SwiftUI",
    "SystemConfiguration",
    "Testing",
    "TipKit",
    "Translation",
    "UIKit",
    "UniformTypeIdentifiers",
    "UserNotifications",
    "VideoToolbox",
    "Vision",
    "WebKit",
    "WidgetKit",
    "XCTest",
    "_Concurrency",
    "_StringProcessing",
    "os",
    "simd",
];

/// Whether `module`, the first segment of an `import`, is a system module.
pub fn is_system(module: &str) -> bool {
    NAMES.binary_search(&module).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_sorted_and_free_of_duplicates() {
        assert!(NAMES.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn system_modules_are_found_and_others_are_not() {
        for module in ["Foundation", "SwiftUI", "AppKit", "XCTest", "Testing", "os"] {
            assert!(is_system(module), "{module}");
        }
        assert!(!is_system("Alamofire"));
        assert!(!is_system("foundation"));
    }

    #[test]
    fn the_design_lists_exactly_these_names() {
        let design = include_str!("../../docs/DESIGN.md");
        let block = design
            .split("**Swift.**")
            .nth(1)
            .and_then(|rest| rest.split("```").nth(1))
            .unwrap();
        let mut listed: Vec<&str> = block.split_whitespace().collect();
        listed.sort_unstable();
        assert_eq!(listed, NAMES);
    }
}
