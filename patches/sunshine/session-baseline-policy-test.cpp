#include "cloudgime_baseline_policy.h"
#include <cassert>
#include <climits>
#include <initializer_list>
int main(){using namespace cloudgime::baseline;
assert(parse_level("0")==0);assert(parse_level("42")==42);for(auto s:{"", "auto", "-1", "42junk", "031", "100"})assert(parse_level(s)==-1);
assert(accepts(0,0,0,0,0,2,1,1));
assert(!accepts(31,720,1570,30,5000,0,0,0));
assert(accepts(32,720,1570,30,5000,0,0,0));
assert(accepts(42,1920,1080,60,20000,0,0,0));
assert(!accepts(42,3840,2160,60,20000,0,0,0));
assert(!accepts(32,720,1570,60,5000,0,0,0));
assert(!accepts(42,720,1570,30,5000,1,0,0));assert(!accepts(42,720,1570,30,5000,0,1,0));assert(!accepts(42,720,1570,30,5000,0,0,1));
assert(!accepts(42,INT_MAX,INT_MAX,INT_MAX,5000,0,0,0));assert(!accepts(42,720,1570,-1,5000,0,0,0));
assert(!accepts(32,720,1570,30,20001,0,0,0));
assert(!accepts(40,8192,16,1,5000,0,0,0));
return 0;}
