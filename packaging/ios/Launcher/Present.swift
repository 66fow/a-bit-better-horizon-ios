import SwiftUI
import UIKit

/// The made-with-Unity clip plays on every app launch, not when Quit Game brings the launcher back.
private var introShown = false

/// The C entry point `horizon-ios-launcher` looks up: show the launcher while the engine is stopped or starting. Play dismisses it; the game's Quit Game asks for it again. `submit` is how the
/// launcher asks the game things (Bridge.swift). Called on the main thread; the presenting waits a turn of the
/// run loop, because at startup the game's view controller may not be on screen yet while its first frame is
/// being set up.
@_cdecl("horizon_launcher_present")
public func horizonLauncherPresent(_ submit: HorizonSubmit?) -> Bool {
    Bridge.submit = submit
    Theme.registerFonts()
    DispatchQueue.main.async { present(attempts: 20) }
    return true
}

private func present(attempts: Int) {
    guard let root = gameViewController(), root.view.window != nil else {
        // Not on screen yet: try again shortly, for a couple of seconds at most.
        if attempts > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { present(attempts: attempts - 1) }
        }
        // Do not leave the resource worker waiting forever if UIKit could not present.
        else { Task { _ = await Bridge.send("launcher-start") } }
        return
    }
    // Already up: a second request (Quit Game pressed twice) changes nothing.
    if root is LauncherHostingController {
        return
    }
    let host = LauncherHostingController(rootView: AnyView(EmptyView()))
    let home = AnyView(
        HomeView(onPlay: { [weak host] in
            guard let host else { return }
            GameAudio.start()
            let scene = host.view.window?.windowScene
            host.dismiss(animated: true) {
                // Back to the game's landscape (its own view controller allows nothing else).
                turn(scene, to: .landscape)
                Task { _ = await Bridge.send("launcher-visible false") }
            }
        })

    )
    // The intro belongs to opening the app: Quit Game brings the launcher back without it.
    if introShown {
        host.rootView = home
    } else {
        introShown = true
        host.introRunning = true
        host.rootView = AnyView(IntroView(onFinish: { [weak host] in
            guard let host, host.introRunning else { return }
            host.introRunning = false
            host.unlockRotation()
            UIView.transition(with: host.view, duration: 0.35, options: .transitionCrossDissolve) {
                host.rootView = home
            }
        }))
    }
    host.modalPresentationStyle = .fullScreen
    host.modalTransitionStyle = .crossDissolve
    root.present(host, animated: false) {
        Task { _ = await Bridge.send("launcher-visible true") }
        // Upright for the intro, and for the launcher's own design on every device.
        turn(host.view.window?.windowScene, to: .portrait)
    }
}

/// Ask the scene to turn to `orientations` now, rather than when the phone is next moved.
func turn(_ scene: UIWindowScene?, to orientations: UIInterfaceOrientationMask) {
    guard let scene else { return }
    if #available(iOS 16.0, *) {
        for window in scene.windows {
            var top = window.rootViewController
            while let next = top?.presentedViewController {
                top = next
            }
            top?.setNeedsUpdateOfSupportedInterfaceOrientations()
        }
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: orientations)) { _ in }
    } else {
        // iOS 15 has no scene geometry request: UIKit turns when a view controller that supports the
        // orientations is presented, and asks again after this nudge.
        UIViewController.attemptRotationToDeviceOrientation()
    }
}

/// The view controller of winit's window: the one the launcher is presented from.
func gameViewController() -> UIViewController? {
    let windows = UIApplication.shared.connectedScenes
        .filter { $0.session.role == .windowApplication }
        .compactMap { $0 as? UIWindowScene }
        .flatMap(\.windows)
    let window = windows.first(where: \.isKeyWindow) ?? windows.first
    var top = window?.rootViewController
    while let presented = top?.presentedViewController {
        top = presented
    }
    return top
}

/// The launcher stands upright, as the design does; the game under it is held sideways by its own view
/// controller, which the phone turns back to when Play dismisses this one. GameInfo.plist allows both.
/// The intro holds it to portrait; once it is done the launcher turns with the device like any screen.
private final class LauncherHostingController: UIHostingController<AnyView> {
    var introRunning = false

    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        introRunning ? .portrait : .all
    }
    override var preferredInterfaceOrientationForPresentation: UIInterfaceOrientation {
        .portrait
    }
    override var prefersStatusBarHidden: Bool { introRunning }
    override var prefersHomeIndicatorAutoHidden: Bool { introRunning }
    override var preferredStatusBarStyle: UIStatusBarStyle { .default }

    /// Lifts the intro's portrait hold, so the launcher may turn with the device from now on.
    func unlockRotation() {
        setNeedsStatusBarAppearanceUpdate()
        setNeedsUpdateOfHomeIndicatorAutoHidden()
        if #available(iOS 16.0, *) {
            setNeedsUpdateOfSupportedInterfaceOrientations()
        } else {
            UIViewController.attemptRotationToDeviceOrientation()
        }
    }
}
