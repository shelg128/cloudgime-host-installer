// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "capture_recovery_policy.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi1_6.h>
#include <chrono>
#include <vector>

namespace cloudgime::capture {
template<class T> struct com_ptr {
 T *p{}; ~com_ptr(){if(p)p->Release();}
 com_ptr()=default; com_ptr(const com_ptr&)=delete; com_ptr& operator=(const com_ptr&)=delete;
 T* operator->()const{return p;}
};
inline std::wstring desktop_name(HDESK desktop) {
 if(!desktop)return {};
 DWORD bytes=0;
 GetUserObjectInformationW(desktop,UOI_NAME,nullptr,0,&bytes);
 if(bytes<sizeof(wchar_t)||bytes>32768)return {};
 std::vector<wchar_t> name(bytes/sizeof(wchar_t)+1,0);
 if(!GetUserObjectInformationW(desktop,UOI_NAME,name.data(),bytes,&bytes))return {};
 return name.data();
}
inline identity read_identity(IDXGIAdapter1 *adapter, IDXGIOutput *output,
                             const DXGI_OUTDUPL_DESC &desc) {
 identity i{}; if(!adapter||!output)return i;
 // Do not call SetThreadDesktop here: recovery must not cross a desktop boundary.
 HDESK input=OpenInputDesktop(0,FALSE,DESKTOP_READOBJECTS);
 if(!input)return i;
 const auto input_name=desktop_name(input);CloseDesktop(input);
 const auto thread_name=desktop_name(GetThreadDesktop(GetCurrentThreadId()));
 if(input_name.empty()||input_name!=thread_name)return i;
 DXGI_ADAPTER_DESC1 ad{};DXGI_OUTPUT_DESC1 od{};
 com_ptr<IDXGIOutput6> o6;
 if(FAILED(adapter->GetDesc1(&ad))||FAILED(output->QueryInterface(__uuidof(IDXGIOutput6),reinterpret_cast<void**>(&o6.p)))||FAILED(o6->GetDesc1(&od)))return i;
 i.desktop=thread_name;i.output=od.DeviceName;i.adapter_high=ad.AdapterLuid.HighPart;i.adapter_low=ad.AdapterLuid.LowPart;
 i.monitor=reinterpret_cast<std::uintptr_t>(od.Monitor);i.attached=od.AttachedToDesktop!=FALSE;
 i.left=od.DesktopCoordinates.left;i.top=od.DesktopCoordinates.top;i.right=od.DesktopCoordinates.right;i.bottom=od.DesktopCoordinates.bottom;
 i.rotation=od.Rotation;i.width=desc.ModeDesc.Width;i.height=desc.ModeDesc.Height;i.format=desc.ModeDesc.Format;
 i.refresh_num=desc.ModeDesc.RefreshRate.Numerator;i.refresh_den=desc.ModeDesc.RefreshRate.Denominator;
 i.color_space=od.ColorSpace;i.bits_per_color=od.BitsPerColor;
 i.valid=i.attached&&i.width>0&&i.height>0&&i.refresh_den>0;
 return i;
}

// Owned by one capture thread. It never owns the encoder/device, switches
// desktop, retries in a loop, or ignores an error. Ineligible/failed recovery
// leaves the caller on its original full-reinit path.
class duplication_recovery {
 identity initial_{}; DXGI_OUTDUPL_DESC desc_{};
 bool attempted_=false, use_output5_=true;std::chrono::steady_clock::time_point last_attempt_{};
 std::vector<DXGI_FORMAT> formats_;
public:
 void initialize(IDXGIAdapter1 *adapter, IDXGIOutput *output, IDXGIOutputDuplication *dup,
                 const std::vector<DXGI_FORMAT> &formats, bool use_output5 = true) {
  initial_={};attempted_=false;formats_=formats;use_output5_=use_output5;if(!dup)return;
  dup->GetDesc(&desc_);initial_=read_identity(adapter,output,desc_);
 }
 const identity& initial()const{return initial_;}
 bool eligible(HRESULT reason,IDXGIFactory1 *factory,IDXGIAdapter1 *adapter,IDXGIOutput *output,ID3D11Device *device)const {
  if(!factory||!device)return false;
  const auto now=std::chrono::steady_clock::now();
  const auto elapsed=attempted_?std::chrono::duration_cast<std::chrono::milliseconds>(now-last_attempt_).count():0;
  return may_recover(static_cast<std::uint32_t>(reason),initial_,read_identity(adapter,output,desc_),
       SUCCEEDED(device->GetDeviceRemovedReason()),factory->IsCurrent()!=FALSE,attempted_,elapsed<0?0:elapsed);
 }
 HRESULT reopen(IDXGIFactory1 *factory, IDXGIAdapter1 *adapter, IDXGIOutput *output,
                ID3D11Device *device, IDXGIOutputDuplication **result) {
  if(!result)return E_POINTER;
  *result=nullptr;
  if(!eligible(DXGI_ERROR_ACCESS_LOST,factory,adapter,output,device))return E_ABORT;
  attempted_=true;last_attempt_=std::chrono::steady_clock::now();
  com_ptr<IDXGIOutputDuplication> next; com_ptr<IDXGIOutput5> output5; HRESULT hr;
  if(use_output5_) {
   hr=output->QueryInterface(__uuidof(IDXGIOutput5),reinterpret_cast<void**>(&output5.p));
   if(FAILED(hr))return hr;
   if(formats_.empty())return E_INVALIDARG;
   hr=output5->DuplicateOutput1(device,0,static_cast<UINT>(formats_.size()),formats_.data(),&next.p);
  } else {
   com_ptr<IDXGIOutput1> output1;hr=output->QueryInterface(__uuidof(IDXGIOutput1),reinterpret_cast<void**>(&output1.p));
   if(FAILED(hr))return hr;
   hr=output1->DuplicateOutput(device,&next.p);
  }
  if(FAILED(hr))return hr;
  DXGI_OUTDUPL_DESC nd{};next->GetDesc(&nd);
  if(!factory->IsCurrent()||FAILED(device->GetDeviceRemovedReason())||!(initial_==read_identity(adapter,output,nd)))return E_ABORT;
  *result=next.p;next.p=nullptr;return S_OK;
 }
};
} // namespace cloudgime::capture
