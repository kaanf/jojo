import Foundation

// M3 (protocol 1.3): the run of an approved lead plan and the project records, as the
// helper reports them. Nothing here is inferred: every field is the helper's answer.

/// The newest run of one plan (`PlanRun`).
public struct PlanRun: Equatable, Sendable {
    public struct Step: Equatable, Sendable, Identifiable {
        public let id: String
        public let key: String
        public let title: String
        public let kind: String?
        public let provider: String
        public let dependsOn: [String]
        /// Waiting, Blocked, Ready, Running, Repairing, Validating, Reviewing, Review,
        /// Accepted, Integrated, Conflict, Failed, NeedsHuman, Refused or Cancelled.
        public let state: String
        public let detail: String
        public let attempt: String?
        public let candidate: String?
        public let receipt: Int64?
        public let commit: String?
        /// M4: repairs so far, the repair the newest attempt is, and its review.
        public var repairs: Int64 = 0
        public var repair: (ordinal: Int64, limit: Int64)? {
            guard let repairOrdinal, let repairLimit else { return nil }
            return (repairOrdinal, repairLimit)
        }
        public var repairOrdinal: Int64?
        public var repairLimit: Int64?
        public var review: StepReview?
    }

    public let plan: String
    public let revision: Int64
    /// RUNNING, PAUSED, CANCELLED, HALTED or DONE.
    public let state: String
    /// The newest control: RUNNING, PAUSED or CANCELLED.
    public let control: String
    public let repository: String
    public let base: String
    public let branch: String
    public let head: String
    public let steps: [Step]

    public init?(_ p: [String: JSON]) {
        guard let plan = p["plan"]?.string, let revision = p["revision"]?.integer, let state = p["state"]?.string,
              let control = p["control"]?.string, let repository = p["repository"]?.string, let base = p["base"]?.string,
              let branch = p["branch"]?.string, let head = p["head"]?.string, let items = p["steps"]?.items else { return nil }
        let steps = items.compactMap { s -> Step? in
            guard let id = s["step"]?.string, let key = s["key"]?.string, let title = s["title"]?.string,
                  let provider = s["provider"]?.string, let state = s["state"]?.string, let detail = s["detail"]?.string,
                  let depends = s["depends_on"]?.items else { return nil }
            var step = Step(id: id, key: key, title: title, kind: s["kind"]?.string, provider: provider,
                            dependsOn: depends.compactMap(\.string), state: state, detail: detail,
                            attempt: s["attempt"]?.string, candidate: s["candidate"]?.string,
                            receipt: s["receipt"]?.integer, commit: s["commit"]?.string)
            step.repairs = s["repairs"]?.integer ?? 0
            (step.repairOrdinal, step.repairLimit) = (s["repair"]?["ordinal"]?.integer, s["repair"]?["limit"]?.integer)
            step.review = s["review"].flatMap(StepReview.init)
            return step
        }
        guard steps.count == items.count else { return nil }
        (self.plan, self.revision, self.state, self.control) = (plan, revision, state, control)
        (self.repository, self.base, self.branch, self.head, self.steps) = (repository, base, branch, head, steps)
    }

    public init(plan: String, revision: Int64, state: String, control: String, repository: String, base: String,
                branch: String, head: String, steps: [Step]) {
        (self.plan, self.revision, self.state, self.control) = (plan, revision, state, control)
        (self.repository, self.base, self.branch, self.head, self.steps) = (repository, base, branch, head, steps)
    }
}

extension PlanRun.Step {
    public init(id: String, key: String, title: String, kind: String? = nil, provider: String, dependsOn: [String] = [],
                state: String, detail: String = "", attempt: String? = nil, commit: String? = nil) {
        self.init(id: id, key: key, title: title, kind: kind, provider: provider, dependsOn: dependsOn, state: state,
                  detail: detail, attempt: attempt, candidate: nil, receipt: nil, commit: commit)
    }
}

/// One project record at its newest revision (`Projects`).
public struct ProjectRecord: Equatable, Sendable, Identifiable {
    public struct KindRule: Equatable, Sendable {
        public let kind: String
        public let provider: String
        public init(kind: String, provider: String) { (self.kind, self.provider) = (kind, provider) }
    }

    public let id: String
    public let revision: Int64
    public let name: String
    public let repositories: [String]
    public let instructions: String
    public let testCommand: [String]
    public let ciCommand: [String]
    public let kinds: [KindRule]
    public let defaultProvider: String
    public let capacity: Int64
    /// M4 (D2): automatic repairs of a failed plan step, 0 to 2, and whether the other
    /// provider reviews a settled step.
    public let repairLimit: Int64
    public let review: Bool

    public init?(_ p: JSON) {
        guard let id = p["project"]?.string, let revision = p["revision"]?.integer, let name = p["name"]?.string,
              let repositories = p["repositories"]?.items, let instructions = p["instructions"]?.string,
              let test = p["test_command"]?.items, let ci = p["ci_command"]?.items, let kinds = p["kinds"]?.items,
              let provider = p["default_provider"]?.string, let capacity = p["capacity"]?.integer,
              let repairLimit = p["repair_limit"]?.integer, let review = p["review"]?.boolean else { return nil }
        let rules = kinds.compactMap { k -> KindRule? in
            guard let kind = k["kind"]?.string, let provider = k["provider"]?.string else { return nil }
            return KindRule(kind: kind, provider: provider)
        }
        guard rules.count == kinds.count else { return nil }
        self.init(id: id, revision: revision, name: name, repositories: repositories.compactMap(\.string),
                  instructions: instructions, testCommand: test.compactMap(\.string), ciCommand: ci.compactMap(\.string),
                  kinds: rules, defaultProvider: provider, capacity: capacity, repairLimit: repairLimit, review: review)
    }

    public init(id: String, revision: Int64, name: String, repositories: [String], instructions: String = "",
                testCommand: [String] = [], ciCommand: [String] = [], kinds: [KindRule] = [],
                defaultProvider: String, capacity: Int64, repairLimit: Int64 = 2, review: Bool = true) {
        (self.id, self.revision, self.name, self.repositories) = (id, revision, name, repositories)
        (self.instructions, self.testCommand, self.ciCommand) = (instructions, testCommand, ciCommand)
        (self.kinds, self.defaultProvider, self.capacity) = (kinds, defaultProvider, capacity)
        (self.repairLimit, self.review) = (repairLimit, review)
    }

    /// The fields of a `SaveProject` for the revision after `revision`.
    public func payload(asNew: Bool) -> [String: JSON] {
        ["project": .text(id), "revision": .int(asNew ? 0 : revision), "name": .text(name),
         "repositories": .list(repositories.map(JSON.text)), "instructions": .text(instructions),
         "test_command": .list(testCommand.map(JSON.text)), "ci_command": .list(ciCommand.map(JSON.text)),
         "kinds": .list(kinds.map { .object(["kind": .text($0.kind), "provider": .text($0.provider)]) }),
         "default_provider": .text(defaultProvider), "capacity": .int(capacity),
         "repair_limit": .int(repairLimit), "review": .bool(review)]
    }
}

/// The answer of `Projects`: every project and the scheduler's capacity now.
public struct ProjectList: Equatable, Sendable {
    public let projects: [ProjectRecord]
    /// The helper-wide capacity (D4): one number, not split by provider.
    public let capacity: Int64

    public init?(_ p: [String: JSON]) {
        guard let items = p["projects"]?.items, let capacity = p["capacity"]?.integer else { return nil }
        let projects = items.compactMap(ProjectRecord.init)
        guard projects.count == items.count else { return nil }
        (self.projects, self.capacity) = (projects, capacity)
    }

    public init(projects: [ProjectRecord], capacity: Int64) {
        (self.projects, self.capacity) = (projects, capacity)
    }
}

extension WorkspaceModel {
    /// The run of every plan held, one `PlanRun` query per plan. A plan never started has
    /// no entry; a read that fails leaves the plan without one.
    func loadPlanRuns() {
        let plans = self.plans.map(\.plan)
        var runs: [String: PlanRun] = [:]
        func next(_ index: Int) {
            guard index < plans.count else {
                planRuns = runs
                onChange?()
                return
            }
            client.query(.PlanRun, ["plan": .text(plans[index])]) { [weak self] reply in
                guard self != nil else { return }
                if case .response(let p) = reply, let run = PlanRun(p) { runs[plans[index]] = run }
                next(index + 1)
            }
        }
        next(0)
    }

    /// Starts the approved plan: the core runs its ready steps up to the capacity.
    public func runPlan(_ plan: String) {
        guard Identity.valid(plan) else { return }
        slice(.RunPlan, ["plan": .text(plan)], sent: "Run sent; awaiting acknowledgement") { [weak self] _ in
            self?.loadPlans()
            return "Plan \(plan.prefix(12)) started; the core starts its ready steps up to the capacity"
        } refused: { "Run refused (\($0)); nothing was started" }
    }

    /// Pause, Resume or Cancel the run of a plan.
    public func controlPlan(_ plan: String, _ action: String) {
        guard Identity.valid(plan) else { return }
        slice(.ControlPlan, ["plan": .text(plan), "action": .text(action)], sent: "\(action) sent; awaiting acknowledgement") { [weak self] p in
            self?.loadPlans()
            return p["response"]?.string ?? "\(action) committed"
        } refused: { "\(action) refused (\($0))" }
    }

    /// Reads the project records again.
    public func loadProjects() {
        guard projectsLoad != .loading, snapshotSeq != nil else { return }
        projectsLoad = .loading
        onChange?()
        client.query(.Projects, [:]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = ProjectList(p) {
                    (self.projects, self.projectsLoad) = (list, .loaded)
                } else {
                    (self.projects, self.projectsLoad) = (nil, .failed("malformed projects"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.projects, self.projectsLoad) = (nil, .failed(reason))
            default:
                (self.projects, self.projectsLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// Saves `record` whole as its next revision (or as a new project).
    public func saveProject(_ record: ProjectRecord, asNew: Bool) {
        slice(.SaveProject, record.payload(asNew: asNew), sent: "Project sent; awaiting acknowledgement") { [weak self] p in
            self?.loadProjects()
            return "Project saved (\(p["response"]?.string ?? "?"))"
        } refused: { "Project refused (\($0)); nothing was saved" }
    }
}
