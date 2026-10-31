import AVFoundation
import SwiftUI
import UIKit

/// The "made with Unity" clip shown once at app launch, before the launcher's home: upright, full
/// screen, silent, and a tap anywhere skips it (Present.swift decides when it runs).
struct IntroView: View {
    var onFinish: () -> Void

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            IntroVideo(onFinish: onFinish)
                .allowsHitTesting(false)
                .ignoresSafeArea()
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: onFinish)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("Skip intro")
    }
}

/// The clip in a layer, played once. A bundle without it stays black for a frame and moves on, so a
/// missing resource can never hold the launcher hostage.
private struct IntroVideo: UIViewRepresentable {
    var onFinish: () -> Void

    func makeUIView(context: Context) -> IntroPlayerView {
        let view = IntroPlayerView()
        view.playerLayer.videoGravity = .resizeAspect
        guard let url = Bundle(for: IntroBundleToken.self)
            .url(forResource: "Unity", withExtension: "mp4")
        else {
            NSLog("Horizon: the intro clip is missing from the launcher framework")
            DispatchQueue.main.async(execute: onFinish)
            return view
        }
        let player = AVPlayer(url: url)
        player.isMuted = true
        view.playerLayer.player = player
        context.coordinator.play(player, finish: onFinish)
        return view
    }

    func updateUIView(_ view: IntroPlayerView, context: Context) {}

    static func dismantleUIView(_ view: IntroPlayerView, coordinator: Coordinator) {
        coordinator.stop()
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject {
        private var player: AVPlayer?
        private var finish: (() -> Void)?
        private var ended = false

        func play(_ player: AVPlayer, finish: @escaping () -> Void) {
            self.player = player
            self.finish = finish
            for name in [AVPlayerItem.didPlayToEndTimeNotification, AVPlayerItem.failedToPlayToEndTimeNotification] {
                NotificationCenter.default.addObserver(
                    self, selector: #selector(itemEnded), name: name, object: player.currentItem
                )
            }
            player.play()
        }

        @objc private func itemEnded() {
            guard !ended else { return }
            ended = true
            let finish = self.finish
            DispatchQueue.main.async { finish?() }
        }

        func stop() {
            NotificationCenter.default.removeObserver(self)
            player?.pause()
            player = nil
            finish = nil
        }
    }
}

private final class IntroPlayerView: UIView {
    override class var layerClass: AnyClass { AVPlayerLayer.self }
    var playerLayer: AVPlayerLayer { layer as! AVPlayerLayer }
}

private final class IntroBundleToken: NSObject {}
