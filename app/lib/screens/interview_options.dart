/// 면접 준비와 설정의 "면접 기본값"이 함께 쓰는 선택지. 두 화면이 같은 저장 값(`Setting.ivYears`, `ivMinutes`,
/// `ivPressure`)을 읽고 쓰므로 값, 이름, 설명을 한 곳에 둔다.
library;

/// 연차, 시간, 면접 분위기 단계의 값과 화면 이름.
const yearOptions = [('new', '신입'), ('junior', '1~3년'), ('senior', '4년 이상')];
const minuteOptions = [(10, '10분'), (12, '12분'), (15, '15분')];
const pressureOptions = [
  ('coaching', '코칭'),
  ('standard', '표준'),
  ('real', '실전'),
];

/// 단계마다 한 줄 설명. 힌트 수는 엔진의 `hints_left`(interview/src/state.rs)와 같다.
const pressureNotes = {
  'coaching': '부드럽게 묻고, 막히면 질문마다 힌트를 한 번 씁니다. 답 뒤에 짚은 핵심 수를 보여 줍니다.',
  'standard': '꼬리질문과 반박을 합니다. 힌트는 주제마다 한 번입니다. 기본값입니다.',
  'real': '짧고 건조하게 묻고 반박을 먼저 합니다. 힌트가 없습니다.',
};

/// 면접 분위기 이름 옆 ⓘ의 글.
const pressureInfo = '면접관의 말투, 반박, 힌트 수를 정합니다. 기본값은 표준입니다.';

/// [options]에서 [value]의 화면 이름.
String optionLabel<T>(List<(T, String)> options, T value) =>
    options.firstWhere((o) => o.$1 == value).$2;
