//! Staged AE2 NumberEntryWidget expression arithmetic.
//!
//! Mirrors the pinned Java shunting-yard parser, decimal division, DECIMAL128
//! external conversion and Java long wrapping. This draft accepts Java BMP
//! decimal digits and a supplied decimal separator; extreme expansions beyond the
//! resource bound are rejected rather than allocating unbounded memory.
const MAX_DIGITS: usize = 4096;
const MAX_TEXT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Decimal { negative: bool, digits: Vec<u8>, scale: usize }

fn trim(v: &mut Vec<u8>) { while v.len()>1 && v.last()==Some(&0) { v.pop(); } }
fn cmp_abs(a:&[u8],b:&[u8])->std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(||a.iter().rev().cmp(b.iter().rev()))
}
fn shift(a:&[u8],n:usize)->Option<Vec<u8>> {
    if a.len().checked_add(n)?>MAX_DIGITS {return None;}
    let mut v=vec![0;n];v.extend_from_slice(a);trim(&mut v);Some(v)
}
fn add_abs(a:&[u8],b:&[u8])->Option<Vec<u8>> {
    let mut out=Vec::with_capacity(a.len().max(b.len())+1);let mut carry=0u8;
    for i in 0..a.len().max(b.len()) {let n=a.get(i).copied().unwrap_or(0)+b.get(i).copied().unwrap_or(0)+carry;out.push(n%10);carry=n/10;}
    if carry>0 {out.push(carry);}if out.len()>MAX_DIGITS{return None;}Some(out)
}
fn sub_abs(a:&[u8],b:&[u8])->Vec<u8> {
    debug_assert!(cmp_abs(a,b)!=std::cmp::Ordering::Less);let mut out=Vec::with_capacity(a.len());let mut borrow=0i8;
    for (i,&digit) in a.iter().enumerate(){let mut n=digit as i8-b.get(i).copied().unwrap_or(0) as i8-borrow;if n<0{n+=10;borrow=1}else{borrow=0}out.push(n as u8);}
    trim(&mut out);out
}
fn mul_abs(a:&[u8],b:&[u8])->Option<Vec<u8>> {
    if a.len().checked_add(b.len())?>MAX_DIGITS+1{return None;}
    let mut raw=vec![0u32;a.len()+b.len()+1];for (i,&x) in a.iter().enumerate(){for (j,&y) in b.iter().enumerate(){raw[i+j]+=u32::from(x)*u32::from(y);}}
    for i in 0..raw.len()-1 {let carry=raw[i]/10;raw[i]%=10;raw[i+1]+=carry;}
    let mut out=raw.into_iter().map(|n|n as u8).collect::<Vec<_>>();trim(&mut out);if out.len()>MAX_DIGITS{return None;}Some(out)
}
fn div_abs(n:&[u8],d:&[u8])->Option<(Vec<u8>,Vec<u8>)> {
    if d==[0]{return None;}let mut rem=vec![0];let mut q=Vec::with_capacity(n.len());
    for &digit in n.iter().rev(){rem.insert(0,digit);trim(&mut rem);let mut v=0;while cmp_abs(&rem,d)!=std::cmp::Ordering::Less {rem=sub_abs(&rem,d);v+=1;}q.push(v);}
    q.reverse();trim(&mut q);Some((q,rem))
}
impl Decimal {
    fn zero()->Self{Self{negative:false,digits:vec![0],scale:0}}
    fn from_i64(n:i64)->Self{let negative=n<0;let mut magnitude=n.unsigned_abs();let mut digits=Vec::new();loop{digits.push((magnitude%10) as u8);magnitude/=10;if magnitude==0{break}}Self{negative,digits,scale:0}}
    fn normalize(mut self)->Self {
        trim(&mut self.digits);while self.scale>0 && self.digits.len()>1 && self.digits[0]==0 {self.digits.remove(0);self.scale-=1;}
        if self.digits==[0] {self.negative=false;self.scale=0;}self
    }
    fn neg(mut self)->Self{if self.digits!=[0]{self.negative=!self.negative;}self}
    fn aligned(&self,other:&Self)->Option<(Vec<u8>,Vec<u8>,usize)> {let scale=self.scale.max(other.scale);Some((shift(&self.digits,scale-self.scale)?,shift(&other.digits,scale-other.scale)?,scale))}
    fn cmp(&self,other:&Self)->Option<std::cmp::Ordering> {
        if self.negative!=other.negative{return Some(if self.negative {std::cmp::Ordering::Less}else{std::cmp::Ordering::Greater});}
        let(a,b,_)=self.aligned(other)?;let order=cmp_abs(&a,&b);Some(if self.negative {order.reverse()}else{order})
    }
    fn add(&self,other:&Self)->Option<Self> {
        let(a,b,scale)=self.aligned(other)?;let (negative,digits)=if self.negative==other.negative{(self.negative,add_abs(&a,&b)?)}else{
            match cmp_abs(&a,&b){std::cmp::Ordering::Less=>(other.negative,sub_abs(&b,&a)),_=>(self.negative,sub_abs(&a,&b))}};
        Some(Self{negative,digits,scale}.normalize())
    }
    fn mul(&self,other:&Self)->Option<Self> {
        let scale=self.scale.checked_add(other.scale)?;if scale>MAX_DIGITS{return None;}
        Some(Self{negative:self.negative^other.negative,digits:mul_abs(&self.digits,&other.digits)?,scale}.normalize())
    }
    fn divide_floor_8(&self,other:&Self)->Option<Self> {
        if other.digits==[0]{return None;}let shift_by=8isize+other.scale as isize-self.scale as isize;
        let(num,den)=if shift_by>=0 {(shift(&self.digits,shift_by as usize)?,other.digits.clone())}else{(self.digits.clone(),shift(&other.digits,(-shift_by) as usize)?)};
        let(mut digits,rem)=div_abs(&num,&den)?;let negative=self.negative^other.negative;
        if negative && rem!=[0]{digits=add_abs(&digits,&[1])?;}
        Some(Self{negative,digits,scale:8}.normalize())
    }
    fn exponent(&self)->Option<u32> {
        if self.negative || self.scale!=0{return None;}let mut n=0u32;for &v in self.digits.iter().rev(){n=n.checked_mul(10)?.checked_add(u32::from(v))?;}if n>30{None}else{Some(n)}
    }
    fn pow(&self,exponent:&Self)->Option<Self> {
        if self.cmp(&Self::from_i64(1_000_000_000))?==std::cmp::Ordering::Greater{return None;}
        let n=exponent.exponent()?;let mut out=Self::from_i64(1);for _ in 0..n{out=out.mul(self)?;}Some(out)
    }
    // BigDecimal multiplication with MathContext.DECIMAL128: 34 significant
    // decimal digits, HALF_EVEN. Restore integer zeros when the drop crosses
    // the decimal point.
    fn precision_34(mut self)->Option<Self> {
        if self.digits.len()<=34{return Some(self);}let drop=self.digits.len()-34;
        let higher=self.digits[drop-1];let lower_nonzero=self.digits[..drop-1].iter().any(|&v|v!=0);let odd=self.digits[drop]%2==1;
        let round=higher>5 || (higher==5 && (lower_nonzero||odd));self.digits.drain(..drop);
        if round {self.digits=add_abs(&self.digits,&[1])?;}
        if drop<=self.scale {self.scale-=drop;}else{self.digits=shift(&self.digits,drop-self.scale)?;self.scale=0;}
        Some(self.normalize())
    }
    // BigDecimal.setScale(0, UP).longValue(): away from zero, then low 64 bits.
    fn external_long(&self,unit:u32)->Option<i64> {
        let mut factor=Vec::new();let mut n=unit;loop{factor.push((n%10) as u8);n/=10;if n==0{break;}}
        let value=self.mul(&Self{negative:false,digits:factor,scale:0})?.precision_34()?;
        let fractional=value.digits.iter().take(value.scale).any(|&v|v!=0);
        let mut result=0u64;for &d in value.digits.iter().skip(value.scale).rev(){result=result.wrapping_mul(10).wrapping_add(u64::from(d));}
        if fractional {result=result.wrapping_add(1);}if value.negative {result=result.wrapping_neg();}Some(result as i64)
    }
    // DecimalFormat("#.######") default HALF_EVEN for button-updated text.
    fn format_6(&self,separator:char)->Option<String> {
        let original_negative=self.negative;let mut value=self.clone();if value.scale>6 {let drop=value.scale-6;let higher=value.digits.get(drop-1).copied().unwrap_or(0);
            let lower=value.digits.iter().take(drop.saturating_sub(1)).any(|&d|d!=0);let odd=value.digits.get(drop).copied().unwrap_or(0)%2==1;
            value.digits=value.digits.into_iter().skip(drop).collect();if value.digits.is_empty(){value.digits.push(0);}value.scale=6;
            if higher>5 || (higher==5 && (lower||odd)){value.digits=add_abs(&value.digits,&[1])?;}value=value.normalize();}
        let mut digits=value.digits.iter().rev().map(|d|char::from(b'0'+*d)).collect::<String>();
        if value.scale>0 {if digits.len()<=value.scale {digits=format!("{}{}","0".repeat(value.scale+1-digits.len()),digits);}digits.insert(digits.len()-value.scale,separator);}
        if value.negative || (original_negative && value.digits==[0]) {digits.insert(0,'-');}Some(digits)
    }
}

#[derive(Clone)]
enum Rpn { Number(Decimal), Op(char) }
fn precedence(c:char)->u8{match c{'^'=>0,'u'=>1,'*'|'/'=>2,'+'|'-'=>3,_=>u8::MAX}}
// NumberFormat recognizes decimal digits beyond the active locale. These BMP
// ranges come from Character.digit(char, 10) in the pinned Java runtime.
fn decimal_digit(c:char)->Option<u8> {
    const ZERO:[u32;37]=[0x0030,0x0660,0x06f0,0x07c0,0x0966,0x09e6,0x0a66,0x0ae6,0x0b66,0x0be6,0x0c66,0x0ce6,0x0d66,0x0de6,0x0e50,0x0ed0,0x0f20,0x1040,0x1090,0x17e0,0x1810,0x1946,0x19d0,0x1a80,0x1a90,0x1b50,0x1bb0,0x1c40,0x1c50,0xa620,0xa8d0,0xa900,0xa9d0,0xa9f0,0xaa50,0xabf0,0xff10];
    let n=c as u32;ZERO.iter().find_map(|&zero|(n>=zero && n<zero+10).then(||(n-zero) as u8))
}
fn java_whitespace(c:char)->bool {
    matches!(c as u32,0x0009..=0x000d|0x001c..=0x0020|0x1680|0x2000..=0x2006|0x2008..=0x200a|0x2028..=0x2029|0x205f|0x3000)
}
fn number_at(chars:&[char],start:usize,separator:char)->Option<(Decimal,usize)> {
    let mut i=start;let mut digits=Vec::new();let mut frac=0usize;let mut seen_separator=false;let mut found=false;
    while let Some(&c)=chars.get(i){if let Some(digit)=decimal_digit(c){found=true;digits.push(digit);if seen_separator{frac+=1;}i+=1;}else if c==separator && !seen_separator {seen_separator=true;i+=1;}else{break}}
    if !found{return None;}let number_end=i;
    let mut exp=0i32;if chars.get(i)==Some(&'E') {let mut j=i+1;let neg=chars.get(j)==Some(&'-');if neg{j+=1;}let begin=j;let mut v=0i32;
        while let Some(&c)=chars.get(j){let Some(digit)=decimal_digit(c) else{break};v=v.checked_mul(10)?.checked_add(i32::from(digit))?;j+=1;}
        if j>begin {exp=if neg{-v}else{v};i=j;}else{i=number_end;}}
    let scale=(frac as i64)-(exp as i64);if scale.unsigned_abs()>MAX_DIGITS as u64{return None;}
    digits.reverse();trim(&mut digits);let mut value=Decimal{negative:false,digits,scale:0};
    if scale>=0 {value.scale=scale as usize;}else{value.digits=shift(&value.digits,(-scale) as usize)?;}
    Some((value.normalize(),i))
}
fn expression(text:&str,separator:char)->Option<Decimal> {
    if text.encode_utf16().count()>MAX_TEXT{return None;}let text=text.strip_prefix('=').unwrap_or(text);let chars=text.chars().collect::<Vec<_>>();let mut out=Vec::new();let mut ops=Vec::new();let mut was=false;let mut i=0;
    while i<chars.len(){if java_whitespace(chars[i]){i+=1;continue;}if !was && chars[i]!='-' {if let Some((v,next))=number_at(&chars,i,separator){out.push(Rpn::Number(v));i=next;was=true;continue;}}
        let op=if chars[i]=='-'&&!was {'u'}else{chars[i]};was=false;match op {
            '('|'u'=>ops.push(op),
            ')'=>{loop{let top=ops.pop()?;if top=='(' {break;}out.push(Rpn::Op(top));}was=true;},
            '+'|'-'|'*'|'/'|'^'=>{while ops.last().is_some_and(|&last|last!='(' && precedence(last)<=precedence(op)){out.push(Rpn::Op(ops.pop()?));}ops.push(op)},
            _=>return None,
        }i+=1;
    }
    while let Some(op)=ops.pop(){out.push(Rpn::Op(op));}let mut stack=Vec::<Decimal>::new();
    for node in out {match node {Rpn::Number(n)=>stack.push(n),Rpn::Op('u')=>{let n=stack.pop()?;stack.push(n.neg());},Rpn::Op(op)=>{let right=stack.pop()?;let left=stack.pop()?;
        let n=match op {'+'=>left.add(&right),'-'=>left.add(&right.neg()),'*'=>left.mul(&right),'/'=>left.divide_floor_8(&right),'^'=>left.pow(&right),_=>None}?;stack.push(n);}}}
    if stack.len()==1{stack.pop()}else{None}
}
/// Mirrors getLongValue: integral unit rejects fractions; conversion wraps as Java long.
pub(super) fn number_value(text:&str,unit:u32,min:i64,max:i64,separator:char)->Option<i64> {
    if unit==0{return None;}let value=expression(text,separator)?;if unit==1 && value.scale>0{return None;}
    let external=value.external_long(unit)?;(external>=min && external<=max).then_some(external)
}
/// Quantity button behavior for unitless/energy fields (both have unit 1).
pub(super) fn number_step(text:&str,delta:i64,min:i64,max:i64,separator:char)->Option<String> {
    let current=expression(text,separator).unwrap_or_else(Decimal::zero);let mut next=current.add(&Decimal::from_i64(delta))?;
    if next.cmp(&Decimal::from_i64(min))?==std::cmp::Ordering::Less{next=Decimal::from_i64(min)}
    else if next.cmp(&Decimal::from_i64(max))?==std::cmp::Ordering::Greater{next=Decimal::from_i64(max)}
    else if current.cmp(&Decimal::from_i64(1))?==std::cmp::Ordering::Equal && delta>1{next=next.add(&Decimal::from_i64(-1))?;}
    Some(next.format_6(separator)?.chars().take(MAX_TEXT).collect())
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn pinned_operator_and_decimal_cases(){
  for (s,n) in [("1+2*3",7),("=-2^2",-4),("2^3^4",4096),("23.4+0.6",24),("2/4*2",1),("1E6",1_000_000),("1--1",2),("--2",2),("64/4",16)] {assert_eq!(number_value(s,1,i64::MIN,i64::MAX,'.'),Some(n),"{s}");}
  for s in ["1/3","1e6","1,000","1 2","2^-1","2^31","1000000001^2","1/0","1++2","1+("] {assert_eq!(number_value(s,1,i64::MIN,i64::MAX,'.'),None,"{s}");}
  assert_eq!(number_value("23,4+0,6",1,i64::MIN,i64::MAX,','),Some(24));
  assert_eq!(number_value("١+٢",1,i64::MIN,i64::MAX,'.'),Some(3));
 }
 #[test] fn java_conversion_rounding_and_wrapping(){
  assert_eq!(number_value("1/3",1000,0,i64::MAX,'.'),Some(334));
  assert_eq!(number_value("-1/6",1000,i64::MIN,i64::MAX,'.'),Some(-167));
  assert_eq!(number_value("1E40",1,i64::MIN,i64::MAX,'.'),Some(-5047021154770878464));
  assert_eq!(number_value("1E99",1,i64::MIN,i64::MAX,'.'),Some(0));
  assert_eq!(number_value(".5",1000,0,i64::MAX,'.'),Some(500));
  assert_eq!(number_value("-1",1,0,i64::MAX,'.'),None);
  assert_eq!(number_value("12345678901234567",1,0,i64::MAX,'.'),None);
 }
 #[test] fn source_button_behavior(){
  assert_eq!(number_step("1",10,i32::MIN as i64,i64::MAX,'.').as_deref(),Some("10"));
  assert_eq!(number_step("1+2",10,0,i64::MAX,'.').as_deref(),Some("13"));
  assert_eq!(number_step("1/3",1,0,i64::MAX,'.').as_deref(),Some("1.333333"));
  assert_eq!(number_step("-2",-1,0,i64::MAX,'.').as_deref(),Some("0"));
  assert_eq!(number_step("-",1,0,i64::MAX,'.').as_deref(),Some("1"));
 }
}
