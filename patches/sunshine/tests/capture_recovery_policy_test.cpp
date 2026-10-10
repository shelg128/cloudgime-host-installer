// SPDX-License-Identifier: GPL-3.0-only
#include "../capture_recovery_policy.h"
#include <iostream>
#include <stdexcept>
using namespace cloudgime::capture;
static void check(bool p,const char* what){if(!p)throw std::runtime_error(what);}
int main(){
 identity a{};a.valid=true;a.attached=true;a.desktop=L"Default";a.output=L"DISPLAY-any";a.monitor=17;a.width=1920;a.height=1080;a.format=87;a.refresh_num=60;a.refresh_den=1;a.bits_per_color=8;
 int checks=0;auto test=[&](bool value,const char* name){check(value,name);++checks;};
 test(may_recover(access_lost,a,a,true,true,false,0),"same identity recovers");
 for(auto e:{0u,0x887A0027u,0x887A002Bu,0x887A0005u,0x80070005u,0x80u})test(!may_recover(e,a,a,true,true,false,0),"only access lost may recover");
 test(!may_recover(access_lost,a,a,false,true,false,0),"device failed");test(!may_recover(access_lost,a,a,true,false,false,0),"stale factory");
 test(!may_recover(access_lost,a,a,true,true,true,249),"rapid failures bounded");test(may_recover(access_lost,a,a,true,true,true,250),"cooldown boundary");
 auto b=a;b.desktop=L"Winlogon";test(!may_recover(access_lost,a,b,true,true,false,0),"secure desktop");b=a;b.output=L"DISPLAY-other";test(!(a==b),"display switch");
 b=a;b.monitor++;test(!(a==b),"monitor handle changed");b=a;b.attached=false;test(!(a==b),"monitor removed");b=a;b.valid=false;test(!(a==b),"unknown state");
 b=a;b.adapter_low++;test(!(a==b),"gpu changed");b=a;b.adapter_high++;test(!(a==b),"gpu luid high changed");b=a;b.width++;test(!(a==b),"width changed");b=a;b.height++;test(!(a==b),"height changed");
 b=a;b.refresh_num=120;test(!(a==b),"refresh numerator changed");b=a;b.refresh_den=1001;test(!(a==b),"refresh denominator changed");
 b=a;b.rotation=3;test(!(a==b),"rotation changed");b=a;b.format=10;test(!(a==b),"HDR pixel format changed");b=a;b.color_space=12;test(!(a==b),"HDR color space changed");b=a;b.bits_per_color=10;test(!(a==b),"HDR bit depth changed");
 b=a;b.left=1;test(!(a==b),"origin left changed");b=a;b.top=1;test(!(a==b),"origin top changed");b=a;b.right=1;test(!(a==b),"right changed");b=a;b.bottom=1;test(!(a==b),"bottom changed");
 identity invalid{};test(!(invalid==invalid),"unknown identity not equal to itself for recovery");
 std::cout<<checks<<" capture recovery policy checks PASS\n";
}
