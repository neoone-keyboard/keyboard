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
