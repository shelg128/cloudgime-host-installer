# SPDX-License-Identifier: GPL-3.0-only
# Modern mingw-w64 headers declare these APIs; older supported headers do not.
include(CheckCXXSourceCompiles)
set(_cg_saved_required_defs "${CMAKE_REQUIRED_DEFINITIONS}")
set(CMAKE_REQUIRED_DEFINITIONS -DWINVER=0x0A00 -D_WIN32_WINNT=0x0A00)
check_cxx_source_compiles("#include <windows.h>
int main() { HSYNTHETICPOINTERDEVICE p = nullptr;
using create_type = decltype(&CreateSyntheticPointerDevice);
using inject_type = decltype(&InjectSyntheticPointerInput);
using destroy_type = decltype(&DestroySyntheticPointerDevice);
return p != nullptr || sizeof(create_type) == 0 || sizeof(inject_type) == 0 || sizeof(destroy_type) == 0;
}" CG_WINDOWS_HAS_SYNTHETIC_POINTER_API)
set(CMAKE_REQUIRED_DEFINITIONS "${_cg_saved_required_defs}")
unset(_cg_saved_required_defs)
if(CG_WINDOWS_HAS_SYNTHETIC_POINTER_API)
  add_compile_definitions(CG_WINDOWS_HAS_SYNTHETIC_POINTER_API=1)
endif()
