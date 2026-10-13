// Rust's additive verification declaration is checked separately from the
// legacy Go fields. Requested Encoding pages are not whole Encoding content.
export function verificationProof(result){
 const sources=result?.sources??[];
 const files=sources.map(source=>{
  const file=source.file,claims=file?.verification;
  const passed=file?.contentVerified===true&&claims?.contentWholeDecodedCKey===true&&claims?.rootWholeDecodedCKey===true&&claims?.encodingFramingEKey===true&&claims?.encodingRequestedPagesMD5===true&&claims?.encodingWholeDecodedCKey===false;
  return {passed,fileDataID:file?.entry?.fileDataId,contentVerified:file?.contentVerified,claims};
 });
 return {passed:files.length>0&&files.every(file=>file.passed),files,scope:'whole selected payload and Root content; authenticated Encoding framing and requested pages; whole Encoding decoded content remains unverified'};
}
