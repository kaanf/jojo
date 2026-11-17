import JojoCore
import SwiftUI

// M5: how a push or a draft pull request is named on the Approval Inbox, Source Control and
// the Recovery Center. Every word follows the helper's state; nothing is shown as pushed
// or opened until the core recorded what was observed.

enum ExternalWords {
    /// The pill of an action, or of its absence ("Not performed").
    static func pill(_ action: ExternalAction?) -> (String, Tone) {
        guard let action else { return ("Not performed", .neutral) }
        switch action.state {
        case .Requested: return ("Requested", .review)
        case .Declined: return ("Declined", .neutral)
        case .InFlight: return (action.kind == .Push ? "Pushing" : "Opening", .info)
        case .Unknown: return ("Outcome unknown", .warn)
        case .Rejected: return ("Rejected", .danger)
        case .Committed:
            if action.kind == .Push { return ("Pushed", .ok) }
            return (action.number.map { "PR #\($0) draft" } ?? "Draft opened", .ok)
        }
    }

    /// "jojo/plan/1a2b3c4d…": a plan branch short enough for a row.
    static func branch(_ name: String) -> String {
        guard name.hasPrefix("jojo/plan/"), name.count > 18 else { return name }
        return String(name.prefix(18)) + "…"
    }

    static func commit(_ id: String) -> String { String(id.prefix(12)) }

    /// The list title of an action.
    static func title(_ action: ExternalAction) -> String {
        action.kind == .Push ? "Push \(branch(action.branch))" : (action.title ?? "Draft pull request")
    }

    /// What the human is asked to authorize, in one sentence.
    static func requested(_ action: ExternalAction) -> String {
        switch action.kind {
        case .Push:
            return "Push commit \(commit(action.commit)) to \(action.remote) as \(action.reference). Nothing else is pushed, and the push is never forced."
        case .PullRequest:
            return "Open a draft pull request from \(action.branch) into \(action.base ?? "the default branch"). It stays a draft; nothing is merged."
        }
    }

    static func risk(_ action: ExternalAction) -> String {
        switch action.kind {
        case .Push:
            return "Publishes the plan branch's commits to the remote, where everyone with access sees them. Jojo does not undo a push. A remote branch at another commit refuses it."
        case .PullRequest:
            return "Creates a draft pull request the repository's collaborators see and are notified of. It can be closed, not unsent."
        }
    }

    /// The confirmation dialog of an authorization.
    static func confirmation(_ action: ExternalAction) -> (title: String, message: String, confirm: String) {
        switch action.kind {
        case .Push:
            return ("Push to \(action.remote)?",
                    "Pushes commit \(action.commit) to \(action.url) as \(action.reference). It leaves this Mac once and is never retried by Jojo.",
                    "Authorize Push")
        case .PullRequest:
            return ("Create a draft pull request?",
                    "Opens a draft pull request from \(action.branch) into \(action.base ?? "the default branch") at \(action.url), titled “\(action.title ?? "")”. It is created once and never retried by Jojo.",
                    "Create Draft PR")
        }
    }

    /// The primary button of a request.
    static func authorize(_ kind: ExternalKind) -> String {
        kind == .Push ? "Authorize Push…" : "Create Draft PR…"
    }

    /// Why an authorization cannot be sent now; nil when it can.
    static func blocked(_ action: ExternalAction, list: ExternalList?, connected: Bool, pending: Bool) -> String? {
        if !connected { return "Unavailable: the core is not connected." }
        if pending { return "Unavailable while a command awaits the core." }
        if action.state != .Requested { return "This request was decided." }
        if action.kind == .PullRequest, let gh = list?.gh, !gh.ready {
            return "gh is not ready: \(gh.detail)"
        }
        return nil
    }

    /// "Ready · credential helper: osxkeychain", for the sign-in rows.
    static func signIn(_ s: ExternalSignIn) -> (String, Tone) {
        switch s.state {
        case "Ready": return ("Ready", .ok)
        case "Missing": return ("Missing", .danger)
        case "NotConfigured": return ("Not configured", .neutral)
        default: return ("Checking", .info)
        }
    }

    static func observer(_ o: ExternalObservation) -> String {
        switch o.observer {
        case "git:push": return "git push"
        case "gh:pr-create": return "gh pr create"
        case "git:ls-remote": return "Inspection (git ls-remote)"
        case "gh:pr-list": return "Inspection (gh pr list)"
        case "human:not-performed": return "You: not performed"
        default: return o.observer
        }
    }
}
