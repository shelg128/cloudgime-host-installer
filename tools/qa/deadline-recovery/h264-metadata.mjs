// Conservative access-unit metadata for already framed Annex-B H264, not a decoder.
export function annexBNals(data) {
 if(!Buffer.isBuffer(data)||data.length<5||data.length>2*1024*1024)throw Error('AU_SIZE');
 const starts=[];for(let i=0;i<data.length-2;i++){if(data[i]===0&&data[i+1]===0){const n=data[i+2]===1?3:i+3<data.length&&data[i+2]===0&&data[i+3]===1?4:0;if(n){starts.push({at:i,payload:i+n});i+=n-1;}}}
 if(!starts.length||starts[0].at!==0)throw Error('ANNEXB_BOUNDARY');if(starts.length>4096)throw Error('NAL_COUNT');
 return starts.map((s,i)=>{let end=starts[i+1]?.at??data.length;while(end>s.payload&&data[end-1]===0)end--;if(end<=s.payload)throw Error('NAL_EMPTY');const header=data[s.payload];if(header&128)throw Error('NAL_FORBIDDEN_BIT');return {type:header&31,referenceIdc:(header>>>5)&3,bytes:end-s.payload,data:data.subarray(s.payload,end)};});
}
export function inspectAccessUnit(data,{boundaryVerified=false}={}) {
 const unknown=reason=>({reference:'unknown',dropSafe:false,reason,dependencyGraphKnown:false,completeSyntaxValidated:false});
 if(boundaryVerified!==true)return unknown('UNVERIFIED_ACCESS_UNIT_BOUNDARY');
 let nals;try{nals=annexBNals(data);}catch(e){return unknown(e.message);}
 // Narrow supported single-view non-partitioned H264 NAL set; others remain unknown.
 if(nals.some(n=>![1,5,6,7,8,9,10,11,12].includes(n.type)))return unknown('UNSUPPORTED_NAL_TYPE');
 const slices=nals.filter(n=>n.type===1||n.type===5),param=nals.some(n=>n.type===7||n.type===8),idr=slices.filter(n=>n.type===5);
 if(!slices.length)return {...unknown(param?'PARAMETER_SET_ONLY':'NO_VIDEO_SLICE'),parameterSets:param};
 if(slices.some(n=>n.bytes<2)||idr.some(n=>n.referenceIdc===0)||(idr.length>0&&idr.length!==slices.length))return unknown('INCONSISTENT_SLICE_HEADERS');
 const allNonRef=slices.every(n=>n.referenceIdc===0),allRef=slices.every(n=>n.referenceIdc>0);
 if(!allNonRef&&!allRef)return unknown('MIXED_REFERENCE_HEADERS');
 const sps=nals.filter(n=>n.type===7).map(n=>n.bytes>=4?{profileIdc:n.data[1],constraints:n.data[2],levelIdc:n.data[3]}:null);
 if(sps.includes(null))return unknown('TRUNCATED_SPS');
 return {reference:idr.length?'key':allRef?'ref':param?'unknown':'nonreference-header',dropSafe:false,parameterSets:param,critical:!!idr.length||allRef||param,vclCount:slices.length,nalCount:nals.length,sps,dependencyGraphKnown:false,completeSyntaxValidated:false,reason:allNonRef?'HEADER_NOT_A_DROP_CERTIFICATE':'PRESERVE_REFERENCE_RECOVERY'};
}
export function readFramedCapture(buffer){if(!Buffer.isBuffer(buffer)||buffer.length>32*1024*1024)throw Error('CAPTURE_SIZE');let pos=0;const frames=[];while(pos<buffer.length){if(pos+20>buffer.length)throw Error('CAPTURE_HEADER');const length=buffer.readUInt32LE(pos);if(length<1||length>2*1024*1024||pos+20+length>buffer.length||frames.length>=5000)throw Error('CAPTURE_LENGTH');const data=buffer.subarray(pos+20,pos+20+length);frames.push({id:frames.length,rtpTimestamp:buffer.readUInt32LE(pos+4),captureUs:buffer.readBigUInt64LE(pos+8).toString(),idr:buffer.readUInt32LE(pos+16),data,metadata:inspectAccessUnit(data,{boundaryVerified:true})});pos+=20+length;}return frames;}
