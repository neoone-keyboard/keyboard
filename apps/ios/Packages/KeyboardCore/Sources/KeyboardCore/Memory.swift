@preconcurrency import Darwin

/// Reads the process's physical memory footprint: the number iOS uses to kill extensions.
public enum MemoryMonitor {
    public static func footprintBytes() -> UInt64 {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(
            MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size
        )
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? info.phys_footprint : 0
    }
}

public enum MemoryPressure: Equatable, Sendable {
    case normal
    /// Drop caches and turn off prediction before iOS kills us.
    case shed
}

public struct MemoryBudget: Sendable {
    public let limitBytes: UInt64
    public let shedFraction: Double

    /// Conservative limit for the keyboard extension, which iOS kills at roughly 48 MB.
    public static let keyboardExtension = MemoryBudget(limitBytes: 40 * 1024 * 1024)

    public init(limitBytes: UInt64, shedFraction: Double = 0.8) {
        self.limitBytes = limitBytes
        self.shedFraction = shedFraction
    }

    public func pressure(footprintBytes: UInt64) -> MemoryPressure {
        Double(footprintBytes) >= Double(limitBytes) * shedFraction ? .shed : .normal
    }
}

/// Turns prediction off when memory runs short and back on once it has
/// recovered. iOS sends keyboards memory warnings whenever the host app is
/// under pressure, so turning off for good would leave the bar empty for hours.
public struct PredictionGate: Sendable {
    public enum Change: Equatable, Sendable { case on, off }

    public let budget: MemoryBudget
    public private(set) var isOn = true
    /// Uptime in seconds of the last warning or overrun.
    private var offSince: Double = 0

    /// Seconds prediction stays off after the last warning or overrun.
    public static let cooldown: Double = 10
    /// Back on only well under the shedding point, so it doesn't flap.
    public static let resumeFraction = 0.6

    public init(budget: MemoryBudget) {
        self.budget = budget
    }

    /// Call on each keystroke; returns a change when prediction should switch.
    public mutating func update(footprintBytes: UInt64, now: Double) -> Change? {
        if isOn {
            guard budget.pressure(footprintBytes: footprintBytes) == .shed else { return nil }
            return turnOff(now: now)
        }
        guard now - offSince >= Self.cooldown,
              Double(footprintBytes) < Double(budget.limitBytes) * Self.resumeFraction
        else { return nil }
        isOn = true
        return .on
    }

    public mutating func memoryWarning(now: Double) -> Change? {
        if !isOn {
            offSince = now
            return nil
        }
        return turnOff(now: now)
    }

    private mutating func turnOff(now: Double) -> Change {
        isOn = false
        offSince = now
        return .off
    }
}
