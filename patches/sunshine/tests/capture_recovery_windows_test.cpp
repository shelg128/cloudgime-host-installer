// SPDX-License-Identifier: GPL-3.0-only
#include "../capture_recovery_windows.h"
#include <iostream>
#include <fstream>
#include <iomanip>
#include <memory>
using namespace cloudgime::capture;
int wmain(int argc,wchar_t**argv){
 if(argc!=3)return 2;
 const bool legacy=std::wstring(argv[2])==L"legacy";
 if(!legacy && std::wstring(argv[2])!=L"output5")return 2;
 std::ofstream out(argv[1]);if(!out)return 3;
 com_ptr<IDXGIFactory1> factory;com_ptr<IDXGIAdapter1> adapter;com_ptr<IDXGIOutput> output;
 auto hr=CreateDXGIFactory1(__uuidof(IDXGIFactory1),reinterpret_cast<void**>(&factory.p));if(FAILED(hr))return 4;
 for(UINT a=0;a<16&&!output.p;++a){com_ptr<IDXGIAdapter1> ad;if(FAILED(factory->EnumAdapters1(a,&ad.p)))break;
  for(UINT o=0;o<16;++o){com_ptr<IDXGIOutput> d;if(FAILED(ad->EnumOutputs(o,&d.p)))break;DXGI_OUTPUT_DESC desc{};
   if(SUCCEEDED(d->GetDesc(&desc))&&desc.AttachedToDesktop){adapter.p=ad.p;ad.p=nullptr;output.p=d.p;d.p=nullptr;break;}}}
 if(!output.p)return 5;
 com_ptr<ID3D11Device> device;com_ptr<ID3D11DeviceContext> context;D3D_FEATURE_LEVEL fl;
 hr=D3D11CreateDevice(adapter.p,D3D_DRIVER_TYPE_UNKNOWN,nullptr,0,nullptr,0,D3D11_SDK_VERSION,&device.p,&fl,&context.p);if(FAILED(hr))return 6;
 com_ptr<IDXGIOutput5> o5;hr=output->QueryInterface(__uuidof(IDXGIOutput5),reinterpret_cast<void**>(&o5.p));if(FAILED(hr))return 7;
 const std::vector<DXGI_FORMAT> formats={DXGI_FORMAT_R16G16B16A16_FLOAT,DXGI_FORMAT_B8G8R8A8_UNORM,DXGI_FORMAT_B8G8R8X8_UNORM,DXGI_FORMAT_R8G8B8A8_UNORM};com_ptr<IDXGIOutputDuplication> dup;
 for(int attempt=0;attempt<3;++attempt){
  if(legacy){com_ptr<IDXGIOutput1> o1;hr=output->QueryInterface(__uuidof(IDXGIOutput1),reinterpret_cast<void**>(&o1.p));if(SUCCEEDED(hr))hr=o1->DuplicateOutput(device.p,&dup.p);}
  else hr=o5->DuplicateOutput1(device.p,0,formats.size(),formats.data(),&dup.p);
  if(SUCCEEDED(hr))break;
  out<<"{\"event\":\"initial_duplicate_error\",\"hr\":\"0x"<<std::hex<<(unsigned)hr<<std::dec<<"\"}\n"<<std::flush;Sleep(200);
 }
 if(FAILED(hr))return 8;
 duplication_recovery recovery;recovery.initialize(adapter.p,output.p,dup.p,formats,!legacy);
 out<<"{\"event\":\"initial\",\"identityValid\":"<<recovery.initial().valid<<"}\n"<<std::flush;
 if(!recovery.initial().valid)return 9;
 auto start=std::chrono::steady_clock::now(),prev=start;auto next=0LL;unsigned frames=0,lost=0,recovered=0,failed=0;long long max_gap=0,max_recovery=0;
 while(std::chrono::steady_clock::now()-start<std::chrono::seconds(45)){
  DXGI_OUTDUPL_FRAME_INFO info{};com_ptr<IDXGIResource> frame;hr=dup->AcquireNextFrame(50,&info,&frame.p);
  if(SUCCEEDED(hr)){++frames;auto now=std::chrono::steady_clock::now();max_gap=std::max(max_gap,(long long)std::chrono::duration_cast<std::chrono::milliseconds>(now-prev).count());prev=now;hr=dup->ReleaseFrame();}
  if(hr==DXGI_ERROR_WAIT_TIMEOUT)continue;
  if(FAILED(hr)){
   auto before=std::chrono::steady_clock::now();++lost;
   if(!recovery.eligible(hr,factory.p,adapter.p,output.p,device.p)){++failed;out<<"{\"event\":\"full_reset_required\",\"hr\":\"0x"<<std::hex<<(unsigned)hr<<std::dec<<"\"}\n";break;}
   if(frame.p){frame.p->Release();frame.p=nullptr;}dup.p->Release();dup.p=nullptr;
   hr=recovery.reopen(factory.p,adapter.p,output.p,device.p,&dup.p);
   if(FAILED(hr)){++failed;out<<"{\"event\":\"full_reset_required\",\"hr\":\"0x"<<std::hex<<(unsigned)hr<<std::dec<<"\"}\n";break;}
   ++recovered;auto ms=std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now()-before).count();max_recovery=std::max(max_recovery,(long long)ms);
   out<<"{\"event\":\"capture_only_recovered\",\"ms\":"<<ms<<"}\n"<<std::flush;
  }
  const auto elapsed=std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now()-start).count();if(elapsed>=next){out<<"{\"event\":\"sample\",\"ms\":"<<elapsed<<",\"frames\":"<<frames<<",\"lost\":"<<lost<<"}\n"<<std::flush;next=elapsed+5000;}
 }
 out<<"{\"event\":\"result\",\"frames\":"<<frames<<",\"accessLost\":"<<lost<<",\"recovered\":"<<recovered<<",\"failed\":"<<failed<<",\"maxFrameGapMs\":"<<max_gap<<",\"maxRecoveryMs\":"<<max_recovery<<"}\n";
 return failed?10:0;
}
