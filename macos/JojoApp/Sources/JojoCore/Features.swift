import Foundation

// Protocol 1.7: what the Dashboard shows about each feature — its stages, what blocks it
// and what waits — as the core derives it. Nothing here is inferred.

/// One lifecycle stage of a feature as `Features` reports it.
public struct FeatureStage: Equatable, Sendable {
    /// Done, Current, Running, NeedsYou, Blocked, Failed, Stale, Unknown or Off.
    public let state: String
    public let label: String?

    public init?(_ p: JSON) {
        guard let state = p["state"]?.string else { return nil }
        self.init(state, p["label"]?.string)
    }

    public init(_ state: String, _ label: String? = nil) {
        (self.state, self.label) = (state, label)
    }

    public static let off = FeatureStage("Off")
}

/// Something that keeps a feature from moving: an open blocking question about it, or a
/// plan step that waits for the human.
public struct FeatureBlocker: Equatable, Sendable, Identifiable {
    /// Question or Step.
    public let kind: String
    public let question: String?
    public let step: String?
    public let key: String?
    public let title: String?
    public let reason: String
    public var id: String { question ?? step ?? reason }

    public init?(_ p: JSON) {
        guard let kind = p["kind"]?.string, let reason = p["reason"]?.string else { return nil }
        self.init(kind: kind, question: p["question"]?.string, step: p["step"]?.string, key: p["key"]?.string,
                  title: p["title"]?.string, reason: reason)
    }

    public init(kind: String, question: String? = nil, step: String? = nil, key: String? = nil,
                title: String? = nil, reason: String) {
        (self.kind, self.question, self.step, self.key, self.title, self.reason) =
            (kind, question, step, key, title, reason)
    }
}

/// One feature as `Features` reports it.
public struct FeatureSummary: Equatable, Sendable, Identifiable {
    public static let stageNames = ["discovery", "specification", "planning", "execution", "verification"]

    public let id: String
    /// Its place in the order features first appeared, from 1.
    public let number: Int64
    public let name: String?
    public let repositoryName: String?
    public let projectName: String?
    /// The project's identity (protocol 1.7); nil when the feature has none.
    public let project: String?
    /// The five stages the core reports, in `stageNames` order.
    public let stages: [FeatureStage]
    public let running: Int64
    public let blocked: Int64
    public let awaiting: Int64
    public let done: Bool
    public let blockers: [FeatureBlocker]

    public init?(_ p: JSON) {
        guard let id = p["feature"]?.string, let number = p["number"]?.integer, let stagesObject = p["stages"],
              let running = p["running"]?.integer, let blocked = p["blocked"]?.integer,
              let awaiting = p["awaiting"]?.integer, let done = p["done"]?.boolean,
              let blockerItems = p["blockers"]?.items else { return nil }
        let stages = Self.stageNames.compactMap { stagesObject[$0].flatMap(FeatureStage.init) }
        let blockers = blockerItems.compactMap(FeatureBlocker.init)
        guard stages.count == Self.stageNames.count, blockers.count == blockerItems.count else { return nil }
        self.init(id: id, number: number, name: p["name"]?.string, repositoryName: p["repository_name"]?.string,
                  projectName: p["project_name"]?.string, stages: stages, running: running, blocked: blocked,
                  awaiting: awaiting, done: done, blockers: blockers, project: p["project"]?.string)
    }

    public init(id: String, number: Int64, name: String? = nil, repositoryName: String? = nil,
                projectName: String? = nil, stages: [FeatureStage] = [], running: Int64 = 0, blocked: Int64 = 0,
                awaiting: Int64 = 0, done: Bool = false, blockers: [FeatureBlocker] = [], project: String? = nil) {
        (self.id, self.number, self.name, self.repositoryName, self.projectName) =
            (id, number, name, repositoryName, projectName)
        self.project = project
        self.stages = stages + Array(repeating: .off, count: max(0, Self.stageNames.count - stages.count))
        (self.running, self.blocked, self.awaiting, self.done, self.blockers) =
            (running, blocked, awaiting, done, blockers)
    }

    /// "FT-12".
    public var code: String { "FT-\(number)" }
}

/// The answer of `Features`: newest first.
public struct FeatureList: Equatable, Sendable {
    public let features: [FeatureSummary]
    public let more: Bool

    public init?(_ p: [String: JSON]) {
        guard let items = p["features"]?.items else { return nil }
        let features = items.compactMap(FeatureSummary.init)
        guard features.count == items.count else { return nil }
        (self.features, self.more) = (features, p["next"]?.integer != nil)
    }

    public init(features: [FeatureSummary], more: Bool = false) {
        (self.features, self.more) = (features, more)
    }
}

extension WorkspaceModel {
    /// Protocol 1.7: the newest features with their stages and blockers. Read only while
    /// the Dashboard or a feature's chat is shown, and again with each new snapshot.
    public func loadFeatures() {
        guard featuresLoad != .loading, snapshotSeq != nil, dashboardShown || chatShown || projectsShown else { return }
        featuresLoad = .loading
        onChange?()
        client.query(.Features, ["before": .null]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = FeatureList(p) {
                    (self.features, self.featuresLoad) = (list, .loaded)
                } else {
                    (self.features, self.featuresLoad) = (nil, .failed("malformed features"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.features, self.featuresLoad) = (nil, .failed(reason))
            default:
                (self.features, self.featuresLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }
}
