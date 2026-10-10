// SPDX-License-Identifier: GPL-3.0-only
// Included once inside namespace platf::dxgi by display_base.cpp.
bool duplication_t::recover_access_lost(HRESULT reason) {
  if (!recovery_display || !recovery.eligible(reason, recovery_display->factory.get(),
      recovery_display->adapter.get(), recovery_display->output.get(), recovery_display->device.get())) {
    return false;
  }
  // The caller must release any resource returned by the failed acquisition.
  // Do not preserve a frame lock or call init(): init() resets the format/encoder.
  has_frame = false;
  dup.reset();
  IDXGIOutputDuplication *replacement = nullptr;
  const auto started = std::chrono::steady_clock::now();
  const auto hr = recovery.reopen(recovery_display->factory.get(), recovery_display->adapter.get(),
      recovery_display->output.get(), recovery_display->device.get(), &replacement);
  if (FAILED(hr)) {
    BOOST_LOG(debug) << "Cloudgime capture-only recovery rejected [0x"sv
                     << util::hex(hr).to_string_view() << "]; full reinit required"sv;
    return false;
  }
  dup.reset(replacement);
  ++recovery_generation;
  auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
      std::chrono::steady_clock::now() - started).count();
  BOOST_LOG(info) << "Cloudgime capture-only recovered; encoder retained; duration_ms="sv << elapsed_ms;
  return true;
}
