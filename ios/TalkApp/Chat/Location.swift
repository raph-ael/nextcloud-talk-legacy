import CoreLocation
import MapKit
import SwiftUI

/// Size of the map in a location bubble (docs/attachments.md "Blase").
let placeMapSize = CGSize(width: 240, height: 150)

/// Map pictures from Apple Maps (`MKMapSnapshotter`), about 1 km wide, kept in memory; light
/// in the dark design too (docs/attachments.md "Blase"). A failure is not remembered: the
/// next try comes when the view shows again.
@MainActor
final class MapSnapshots {
    static let shared = MapSnapshots()

    private let cache: NSCache<NSString, UIImage> = {
        let c = NSCache<NSString, UIImage>()
        c.totalCostLimit = 24 * 1024 * 1024
        return c
    }()

    private func key(_ lat: Double, _ lon: Double, _ size: CGSize) -> NSString {
        String(format: "%.6f,%.6f,%.0fx%.0f", lat, lon, size.width, size.height) as NSString
    }

    func cached(_ lat: Double, _ lon: Double, _ size: CGSize) -> UIImage? {
        cache.object(forKey: key(lat, lon, size))
    }

    func snapshot(_ lat: Double, _ lon: Double, _ size: CGSize) async -> UIImage? {
        if let hit = cached(lat, lon, size) { return hit }
        let options = MKMapSnapshotter.Options()
        let center = CLLocationCoordinate2D(latitude: lat, longitude: lon)
        options.region = MKCoordinateRegion(
            center: center, latitudinalMeters: 1000 * size.height / size.width, longitudinalMeters: 1000)
        options.size = size
        let scale = UIScreen.main.scale
        options.traitCollection = UITraitCollection { t in
            t.userInterfaceStyle = .light
            t.displayScale = scale
        }
        guard let shot = try? await MKMapSnapshotter(options: options).start() else { return nil }
        let image = shot.image
        let cost = Int(image.size.width * image.scale * image.size.height * image.scale * 4)
        cache.setObject(image, forKey: key(lat, lon, size), cost: cost)
        return image
    }
}

/// The pin: a round head in `accent` with its tip at the bottom centre.
private struct PinShape: Shape {
    func path(in r: CGRect) -> Path {
        let radius = r.width / 2
        let c = CGPoint(x: r.midX, y: r.minY + radius)
        var p = Path()
        // The sides leave the circle where tangents from the tip touch it.
        let tip = CGPoint(x: r.midX, y: r.maxY)
        let d = tip.y - c.y
        let a = acos(radius / d)
        p.move(to: tip)
        p.addArc(center: c, radius: radius, startAngle: .radians(.pi / 2 + a), endAngle: .radians(.pi / 2 - a),
                 clockwise: false)
        p.closeSubpath()
        return p
    }
}

struct MapPin: View {
    static let size = CGSize(width: 26, height: 36)

    var body: some View {
        PinShape()
            .fill(Talk.accent)
            .overlay(alignment: .top) {
                Circle().fill(.white).frame(width: 9, height: 9).padding(.top, 8.5)
            }
            .frame(width: Self.size.width, height: Self.size.height)
            .shadow(color: .black.opacity(0.3), radius: 2, y: 1)
            // The tip is the centre of the map.
            .offset(y: -Self.size.height / 2)
            .accessibilityHidden(true)
    }
}

/// Map picture of a place with the pin on it; a `chip` area with the pin while loading,
/// offline or after an error.
struct PlaceMap: View {
    let latitude: Double
    let longitude: Double
    let size: CGSize

    @State private var image: UIImage?
    @State private var shownKey = ""

    var body: some View {
        let key = "\(latitude),\(longitude)"
        let shown = (shownKey == key ? image : nil) ?? MapSnapshots.shared.cached(latitude, longitude, size)
        ZStack {
            Talk.chip
            if let shown {
                Image(uiImage: shown).resizable().scaledToFill()
            }
            MapPin()
        }
        .frame(width: size.width, height: size.height)
        .clipped()
        .task(id: key) {
            guard MapSnapshots.shared.cached(latitude, longitude, size) == nil else { return }
            if let img = await MapSnapshots.shared.snapshot(latitude, longitude, size), !Task.isCancelled {
                image = img
                shownKey = key
            }
        }
    }
}

enum Places {
    /// Apple Maps with a pin and the name.
    static func openInMaps(_ l: GeoLocation) {
        let item = MKMapItem(placemark: MKPlacemark(
            coordinate: CLLocationCoordinate2D(latitude: l.latitude, longitude: l.longitude)))
        item.name = Format.placeName(l)
        item.openInMaps(launchOptions: [
            MKLaunchOptionsMapSpanKey: NSValue(mkCoordinateSpan: MKCoordinateSpan(latitudeDelta: 0.01, longitudeDelta: 0.01)),
        ])
    }

    /// What "Kopieren" puts on the clipboard: the name and the link below it.
    static func copyText(_ l: GeoLocation) -> String {
        Format.placeName(l) + "\n" + Format.placeLink(l)
    }
}

// MARK: - Sending

/// The own position while the sheet is open (never in the background): permission,
/// updates, a 30 s limit for the first fix and the address from `CLGeocoder`.
@MainActor
@Observable
final class PlaceFinder: NSObject, CLLocationManagerDelegate {
    enum State: Equatable {
        case searching
        case denied
        case off
        case notFound
        case found
    }

    private(set) var state: State = .searching
    private(set) var location: CLLocation?
    /// The address once known; nil while asking and after it failed ("Standort" is shown).
    private(set) var address: String?

    @ObservationIgnored private let manager = CLLocationManager()
    @ObservationIgnored private let geocoder = CLGeocoder()
    @ObservationIgnored private var timeout: Task<Void, Never>?
    @ObservationIgnored private var geocodeLimit: Task<Void, Never>?
    @ObservationIgnored private var geocoded: CLLocation?
    @ObservationIgnored private var running = false

    override init() {
        super.init()
        manager.desiredAccuracy = kCLLocationAccuracyBest
        manager.delegate = self
    }

    /// Bezeichnung shown and sent.
    var name: String { address ?? "Standort" }

    func start() {
        running = true
        state = .searching
        location = nil
        address = nil
        geocoded = nil
        Task {
            let enabled = await Task.detached { CLLocationManager.locationServicesEnabled() }.value
            guard running else { return }
            if !enabled {
                state = .off
                return
            }
            evaluate()
        }
    }

    func stop() {
        running = false
        manager.stopUpdatingLocation()
        timeout?.cancel()
        geocodeLimit?.cancel()
        geocoder.cancelGeocode()
    }

    private func evaluate() {
        guard running else { return }
        switch manager.authorizationStatus {
        case .notDetermined:
            manager.requestWhenInUseAuthorization()
        case .denied, .restricted:
            manager.stopUpdatingLocation()
            timeout?.cancel()
            state = .denied
        default:
            manager.startUpdatingLocation()
            if location == nil {
                state = .searching
                armTimeout()
            }
        }
    }

    private func armTimeout() {
        timeout?.cancel()
        timeout = Task { [weak self] in
            try? await Task.sleep(for: .seconds(30))
            guard let self, !Task.isCancelled, self.running, self.location == nil else { return }
            self.manager.stopUpdatingLocation()
            self.state = .notFound
        }
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        Task { @MainActor in self.evaluate() }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        Task { @MainActor in
            guard self.running else { return }
            for l in locations where l.horizontalAccuracy >= 0 { self.take(l) }
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        Task { @MainActor in
            guard self.running, (error as? CLError)?.code == .denied else { return }
            self.state = .denied
            self.timeout?.cancel()
        }
    }

    /// A more accurate (or a clearly newer) fix replaces the shown one.
    private func take(_ l: CLLocation) {
        if let old = location, l.horizontalAccuracy > old.horizontalAccuracy,
           l.timestamp.timeIntervalSince(old.timestamp) < 10 { return }
        location = l
        state = .found
        timeout?.cancel()
        if geocoded.map({ $0.distance(from: l) > 30 }) ?? true { lookUp(l) }
    }

    /// The address, at most 5 s; a later fix asks again once it moved more than 30 m.
    private func lookUp(_ l: CLLocation) {
        geocoded = l
        geocoder.cancelGeocode()
        geocodeLimit?.cancel()
        geocodeLimit = Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            guard let self, !Task.isCancelled else { return }
            self.geocoder.cancelGeocode()
        }
        geocoder.reverseGeocodeLocation(l, preferredLocale: Locale(identifier: "de_DE")) { [weak self] marks, _ in
            Task { @MainActor in
                guard let self, self.running, self.geocoded === l else { return }
                self.geocodeLimit?.cancel()
                if let mark = marks?.first, let text = Self.address(mark) { self.address = text }
            }
        }
    }

    /// "{Straße} {Hausnummer}, {Ort}", without a street "{Ort}".
    nonisolated static func address(_ p: CLPlacemark) -> String? {
        let street = [p.thoroughfare, p.subThoroughfare].compactMap { $0 }.joined(separator: " ")
        let town = p.locality ?? ""
        switch (street.isEmpty, town.isEmpty) {
        case (false, false): return "\(street), \(town)"
        case (true, false): return town
        case (false, true): return street
        default: return nil
        }
    }
}

/// "Standort senden" (docs/attachments.md "Senden"): map with pin, name, accuracy, one button.
struct PlaceSendSheet: View {
    let onSend: (GeoLocation) -> Void
    let onClose: () -> Void

    @State private var finder = PlaceFinder()
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                Text("Standort senden").font(.system(size: 17, weight: .semibold))
                HStack {
                    Spacer()
                    Button(action: onClose) {
                        Image(systemName: "xmark")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(Talk.muted)
                            .frame(width: 32, height: 32)
                            .background(Talk.chip, in: Circle())
                    }
                    .accessibilityLabel("Schließen")
                    .accessibilityIdentifier("place-close")
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 16)
            .padding(.bottom, 14)
            content
            Spacer(minLength: 0)
        }
        .foregroundStyle(Talk.onSurface)
        .background(Talk.background)
        .onAppear { finder.start() }
        .onDisappear { finder.stop() }
    }

    @ViewBuilder
    private var content: some View {
        let mapSize = CGSize(width: 343, height: 190)
        switch finder.state {
        case .denied:
            message("Wolkenfunk darf den Standort nicht verwenden", button: "Einstellungen öffnen") {
                if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
            }
        case .off:
            message("Die Ortung ist ausgeschaltet", button: nil) {}
        case .notFound:
            message("Standort nicht gefunden", button: "Erneut versuchen") { finder.start() }
        case .searching, .found:
            VStack(alignment: .leading, spacing: 12) {
                Group {
                    if let l = finder.location {
                        PlaceMap(latitude: l.coordinate.latitude, longitude: l.coordinate.longitude, size: mapSize)
                    } else {
                        ZStack {
                            Talk.chip
                            HStack(spacing: 8) {
                                ProgressView()
                                Text("Standort wird bestimmt …").foregroundStyle(Talk.muted)
                            }
                        }
                        .frame(width: mapSize.width, height: mapSize.height)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 14))
                .accessibilityIdentifier("place-map")
                VStack(alignment: .leading, spacing: 2) {
                    Text(finder.location == nil ? " " : finder.name)
                        .font(.system(size: 16, weight: .medium))
                        .lineLimit(2)
                        .accessibilityIdentifier("place-name")
                    Text(finder.location.map { Format.accuracy($0.horizontalAccuracy) } ?? " ")
                        .font(.system(size: 13))
                        .foregroundStyle(Talk.muted)
                        .accessibilityIdentifier("place-accuracy")
                }
                Button {
                    guard let l = finder.location else { return }
                    finder.stop()
                    onSend(GeoLocation(latitude: l.coordinate.latitude, longitude: l.coordinate.longitude, name: finder.name))
                } label: {
                    Text("Diesen Standort senden")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(Talk.accent.opacity(finder.location == nil ? 0.4 : 1), in: Capsule())
                }
                .buttonStyle(.plain)
                .disabled(finder.location == nil)
                .accessibilityIdentifier("place-send")
                .padding(.top, 6)
            }
            .frame(width: mapSize.width)
        }
    }

    private func message(_ text: String, button: String?, action: @escaping () -> Void) -> some View {
        VStack(spacing: 14) {
            Image(systemName: "location.slash")
                .font(.system(size: 30))
                .foregroundStyle(Talk.muted)
            Text(text)
                .font(.system(size: 16))
                .multilineTextAlignment(.center)
                .accessibilityIdentifier("place-message")
            if let button {
                Button(button, action: action)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(Talk.accent)
                    .accessibilityIdentifier("place-action")
            }
        }
        .padding(.horizontal, 24)
        .padding(.top, 40)
    }
}
