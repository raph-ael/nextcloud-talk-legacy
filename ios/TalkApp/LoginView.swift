import SafariServices
import SwiftUI

/// Login via Nextcloud Login Flow v2: the user signs in (incl. 2FA) in a Safari sheet,
/// the core polls in the background and returns an app password.
struct LoginView: View {
    let onLogin: (LoginResult) throws -> Void

    @State private var server = ""
    @State private var page: LoginPage?
    @State private var flow: LoginFlow?
    @State private var flowTask: Task<Void, Never>?
    @State private var error: String?

    private var busy: Bool { flowTask != nil }

    var body: some View {
        VStack(spacing: 20) {
            Spacer()
            Image(systemName: "bubble.left.and.bubble.right.fill")
                .font(.system(size: 64))
                .foregroundStyle(.tint)
            Text("Willkommen")
                .font(.largeTitle.bold())
            Text("Melde dich mit deinem Nextcloud-Konto an.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            TextField("cloud.example.org", text: $server)
                .textContentType(.URL)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.go)
                .onSubmit(start)
                .padding(14)
                .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 14))
                .padding(.top, 12)

            Button(action: start) {
                Group {
                    if busy { ProgressView() } else { Text("Anmelden") }
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(server.trimmingCharacters(in: .whitespaces).isEmpty || busy)

            if let error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
            Spacer()
            Spacer()
        }
        .padding(24)
        .sheet(item: $page, onDismiss: cancelIfPending) { page in
            SafariView(url: page.url).ignoresSafeArea()
        }
    }

    private func start() {
        guard !busy else { return }
        error = nil
        let server = server
        flowTask = Task {
            defer { flowTask = nil; flow = nil }
            do {
                let started = try await LoginFlow.start(server: server, platform: .ios)
                flow = started
                guard let url = URL(string: started.loginUrl()) else { throw URLError(.badURL) }
                page = LoginPage(url: url)
                let result = try await started.wait()
                page = nil
                try onLogin(result)
            } catch CoreError.Cancelled {
                // User closed the browser sheet.
            } catch {
                page = nil
                self.error = Self.describe(error)
            }
        }
    }

    /// Closing the Safari sheet before approving stops polling.
    private func cancelIfPending() {
        flow?.cancel()
    }

    private static func describe(_ error: Error) -> String {
        guard let error = error as? CoreError else {
            return "Anmeldung fehlgeschlagen: \(error.localizedDescription)"
        }
        switch error {
        case .Network: return "Server nicht erreichbar. Adresse prüfen?"
        case .Unauthorized: return "Anmeldung abgelehnt."
        case .Server(let message): return "Serverfehler: \(message)"
        case .Cancelled: return ""
        }
    }
}

private struct LoginPage: Identifiable {
    let url: URL
    var id: URL { url }
}

struct SafariView: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> SFSafariViewController {
        SFSafariViewController(url: url)
    }

    func updateUIViewController(_ controller: SFSafariViewController, context: Context) {}
}
