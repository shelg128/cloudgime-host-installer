// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
#include <string>

namespace cloudgime::capture {
struct identity {
  bool valid{};
  std::wstring desktop, output;
  std::int32_t adapter_high{}, left{}, top{}, right{}, bottom{};
  std::uint32_t adapter_low{}, rotation{}, refresh_num{}, refresh_den{}, width{}, height{}, format{}, color_space{}, bits_per_color{};
  std::uintptr_t monitor{};
  bool attached{};
  bool operator==(const identity &b) const {
    return valid && b.valid && attached && b.attached && desktop == b.desktop && output == b.output &&
      adapter_high == b.adapter_high && adapter_low == b.adapter_low && monitor == b.monitor &&
      left == b.left && top == b.top && right == b.right && bottom == b.bottom &&
      rotation == b.rotation && refresh_num == b.refresh_num && refresh_den == b.refresh_den &&
      width == b.width && height == b.height && format == b.format && color_space == b.color_space &&
      bits_per_color == b.bits_per_color;
  }
};
// Only ACCESS_LOST is recoverable here. Access denied, device removal, desktop
// switches, unknown state and rapid repeated loss must take the full reset path.
inline constexpr std::uint32_t access_lost = 0x887A0026u;
inline constexpr std::uint64_t retry_floor_ms = 250;
inline bool may_recover(std::uint32_t error, const identity &initial, const identity &current,
                        bool device_healthy, bool factory_current, bool attempted,
                        std::uint64_t elapsed_since_attempt_ms) {
  return error == access_lost && device_healthy && factory_current && initial == current &&
    (!attempted || elapsed_since_attempt_ms >= retry_floor_ms);
}
} // namespace cloudgime::capture
