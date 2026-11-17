import Foundation

/// D3: one executable as the core recorded it with a run.
public struct ExecutableRun: Equatable, Sendable {
    public let path: String
    public let version: String
    public let sha256: String
    /// The version is the one the stream profiles were written against.
    public let profiled: Bool
    /// The event that first recorded it.
    public let event: Int64

    public init?(_ json: JSON) {
        guard let path = json["path"]?.string, let version = json["version"]?.string,
              let sha256 = json["sha256"]?.string, let profiled = json["profiled"]?.boolean,
              let event = json["event"]?.integer else { return nil }
        (self.path, self.version, self.sha256, self.profiled, self.event) = (path, version, sha256, profiled, event)
    }

    public init(path: String, version: String, sha256: String, profiled: Bool, event: Int64) {
        (self.path, self.version, self.sha256, self.profiled, self.event) = (path, version, sha256, profiled, event)
    }
}

/// D3: a provider's executable changed between two runs. Shown as a warning; the run was
/// not refused.
public struct ExecutableChange: Equatable, Sendable {
    public let event: Int64
    public let previous: ExecutableRun
    public let current: ExecutableRun

    public init(event: Int64, previous: ExecutableRun, current: ExecutableRun) {
        (self.event, self.previous, self.current) = (event, previous, current)
    }
}

/// D3: what the core knows about one provider's executable (the `Executables` query).
public struct ProviderExecutable: Equatable, Sendable {
    /// How the core knows the executable.
    public enum Source: String, Sendable {
        case discovered, path, pinned, none
    }
    public enum State: String, Sendable {
        case ready, checking, refused, unconfigured
    }

    public let provider: String
    public let source: Source
    public let state: State
    /// Why it cannot be run; empty unless refused.
    public let detail: String
    /// The PATH entry it was found under; nil unless discovered.
    public let entry: String?
    /// The native binary that runs, its version and digest, as the core knows them now.
    public let path: String?
    public let version: String?
    public let sha256: String?
    public let profiled: Bool?
    public let lastRun: ExecutableRun?
    public let change: ExecutableChange?

    public init?(_ json: JSON) {
        guard let provider = json["provider"]?.string,
              let source = json["source"]?.string.flatMap(Source.init(rawValue:)),
              let state = json["state"]?.string.flatMap(State.init(rawValue:)),
              let detail = json["detail"]?.string else { return nil }
        func optional<T>(_ key: String, _ read: (JSON) -> T?) -> T?? {
            guard let value = json[key] else { return .none }
            if value.isNull { return .some(nil) }
            return read(value).map { .some($0) } ?? .none
        }
        guard let entry = optional("entry", \.string), let path = optional("path", \.string),
              let version = optional("version", \.string), let sha256 = optional("sha256", \.string),
              let profiled = optional("profiled", \.boolean),
              let lastRun = optional("last_run", ExecutableRun.init),
              let change = optional("change", { value -> ExecutableChange? in
                  guard let event = value["event"]?.integer, let previous = value["previous"].flatMap(ExecutableRun.init),
                        let current = value["current"].flatMap(ExecutableRun.init) else { return nil }
                  return ExecutableChange(event: event, previous: previous, current: current)
              }) else { return nil }
        (self.provider, self.source, self.state, self.detail) = (provider, source, state, detail)
        (self.entry, self.path, self.version, self.sha256, self.profiled) = (entry, path, version, sha256, profiled)
        (self.lastRun, self.change) = (lastRun, change)
    }

    public init(provider: String, source: Source, state: State, detail: String = "", entry: String? = nil,
                path: String? = nil, version: String? = nil, sha256: String? = nil, profiled: Bool? = nil,
                lastRun: ExecutableRun? = nil, change: ExecutableChange? = nil) {
        (self.provider, self.source, self.state, self.detail) = (provider, source, state, detail)
        (self.entry, self.path, self.version, self.sha256, self.profiled) = (entry, path, version, sha256, profiled)
        (self.lastRun, self.change) = (lastRun, change)
    }
}

/// D3: the answer of the `Executables` query, one entry per provider.
public struct ExecutableList: Equatable, Sendable {
    public let providers: [ProviderExecutable]

    public init?(_ p: [String: JSON]) {
        guard let items = p["providers"]?.items else { return nil }
        let providers = items.compactMap(ProviderExecutable.init)
        guard providers.count == items.count else { return nil }
        self.providers = providers
    }

    public init(providers: [ProviderExecutable]) { self.providers = providers }

    /// The entry of `provider` ("ClaudeCode" or "Codex").
    public subscript(provider: String) -> ProviderExecutable? { providers.first { $0.provider == provider } }
}

extension WorkspaceModel {
    /// D3: reads what the core knows about each provider's executable. Asked only while
    /// Settings & Providers is shown; asking may start a check in the core, never a run.
    public func loadExecutables() {
        guard executablesLoad != .loading, snapshotSeq != nil, executablesShown else { return }
        executablesLoad = .loading
        onChange?()
        client.query(.Executables, [:]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = ExecutableList(p) {
                    (self.executables, self.executablesLoad) = (list, .loaded)
                    // A check the core is still running is asked about again shortly.
                    if self.executablesShown, list.providers.contains(where: { $0.state == .checking }) {
                        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in self?.loadExecutables() }
                    }
                } else {
                    (self.executables, self.executablesLoad) = (nil, .failed("malformed executables"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.executables, self.executablesLoad) = (nil, .failed(reason))
            default:
                (self.executables, self.executablesLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }
}
