/// 화면을 짜는 동안 쓰는 견본 자료. 인물과 회사는 지어낸 것이다.
/// 엔진이 붙으면 이 파일은 없어진다.
library;

import 'design/components.dart';

class Claim {
  const Claim(this.number, this.text, this.status, {this.note});

  final int number;
  final String text;
  final ClaimStatus status;

  /// 방어하지 못한 이유나 확인이 필요한 점.
  final String? note;
}

enum ClaimStatus {
  defended('방어함', Tone.success),
  partial('일부 방어', Tone.warning),
  failed('방어 못 함', Tone.danger),
  untested('아직 안 물음', Tone.neutral);

  const ClaimStatus(this.label, this.tone);
  final String label;
  final Tone tone;
}

const applicant = '김서연';

const claims = [
  Claim(
    1,
    'Kafka 기반 이벤트 파이프라인을 설계해 주문 처리 지연을 p95 기준 1.2초에서 300ms로 줄였습니다.',
    ClaimStatus.defended,
  ),
  Claim(
    2,
    '결제 API의 멱등성을 보장하기 위해 요청 키와 상태 머신을 함께 사용했습니다.',
    ClaimStatus.failed,
    note: '같은 키의 요청이 동시에 들어올 때를 설명하지 못했습니다.',
  ),
  Claim(
    3,
    'Redis 캐시 무효화 전략을 TTL과 이벤트 기반으로 나누어 적용했습니다.',
    ClaimStatus.partial,
    note: '이벤트가 유실될 때의 대책이 빠졌습니다.',
  ),
  Claim(4, 'PostgreSQL 실행 계획을 분석해 느린 쿼리 37건을 제거했습니다.', ClaimStatus.defended),
  Claim(
    5,
    '모놀리식 서비스를 6개의 서비스로 분리하는 작업을 주도했습니다.',
    ClaimStatus.untested,
    note: '"주도"의 범위를 물을 수 있습니다. 직접 결정한 부분을 준비하세요.',
  ),
];

class Posting {
  const Posting(
    this.company,
    this.role,
    this.level,
    this.daysLeft,
    this.defended,
    this.total,
  );

  final String company;
  final String role;
  final String level;
  final int daysLeft;
  final int defended;
  final int total;
}

const postings = [
  Posting('예시페이', '백엔드 엔지니어', '신입', 9, 2, 5),
  Posting('모아커머스', '서버 개발자', '신입 ~ 3년', 21, 0, 5),
];

class DocItem {
  const DocItem(this.name, this.kind, this.edited, this.pages, this.claimCount);

  final String name;
  final String kind;
  final String edited;
  final int pages;
  final int claimCount;
}

const documents = [
  DocItem('이력서', '흐름 문서', '오늘 14:20', 2, 5),
  DocItem('경력기술서', '흐름 문서', '어제', 3, 0),
  DocItem('포트폴리오', '캔버스 문서', '11월 8일', 12, 0),
];

/// 면접 주제와 면별 상태. 순서는 원리, 한계, 대안, 검증이다.
class Topic {
  const Topic(this.name, this.facets, {this.current = false});

  final String name;
  final List<FacetState> facets;
  final bool current;
}

const facetNames = ['원리', '한계', '대안', '검증'];

const topics = [
  Topic('메시지 전달 보장', [
    FacetState.reached,
    FacetState.reached,
    FacetState.untested,
    FacetState.reached,
  ]),
  Topic('멱등성', [
    FacetState.reached,
    FacetState.missed,
    FacetState.untested,
    FacetState.untested,
  ], current: true),
  Topic('캐시 무효화', [
    FacetState.reached,
    FacetState.missed,
    FacetState.reached,
    FacetState.untested,
  ]),
  Topic('인덱스와 실행 계획', [
    FacetState.reached,
    FacetState.reached,
    FacetState.reached,
    FacetState.reached,
  ]),
  Topic('트랜잭션 격리 수준', [
    FacetState.untested,
    FacetState.untested,
    FacetState.untested,
    FacetState.untested,
  ]),
];

const question = '같은 요청 키로 두 요청이 동시에 들어오면 어떻게 됩니까?';

const transcript =
    '요청 키를 유일 인덱스로 저장하고 있어서 두 번째 요청은 삽입 단계에서 실패합니다. '
    '실패한 요청은 첫 번째 요청의 상태를 조회해서, 처리 중이면 잠시 뒤에 다시 시도하라는 응답을 주고 '
    '완료 상태면 저장해 둔 결과를 그대로 돌려줍니다.';

class Fix {
  const Fix(this.claim, this.title, this.clip, this.missing, this.next);

  final int claim;
  final String title;
  final String clip;
  final String missing;
  final String next;
}

const fixes = [
  Fix(
    2,
    '동시에 들어온 같은 요청을 설명하지 못했습니다',
    '02:14 ~ 03:05',
    '첫 요청이 처리 중일 때 두 번째 요청이 받는 응답이 빠졌습니다.',
    '요청 키에 유일 제약을 걸었는지, 처리 중 상태를 어떻게 돌려주는지 한 문장으로 정리하세요.',
  ),
  Fix(
    3,
    '이벤트가 유실될 때의 대책이 없었습니다',
    '06:40 ~ 07:32',
    '이벤트 기반 무효화가 실패하면 캐시가 얼마나 오래 틀린 값을 주는지 말하지 않았습니다.',
    'TTL이 마지막 안전장치라는 점과 그 길이를 정한 근거를 준비하세요.',
  ),
  Fix(
    1,
    '결론이 늦게 나왔습니다',
    '00:48 ~ 01:55',
    '지연이 줄어든 이유를 말하기 전에 배경 설명이 40초 이어졌습니다.',
    '"병목은 동기 호출이었고, 비동기로 바꿔 줄였습니다"로 시작하세요.',
  ),
];
