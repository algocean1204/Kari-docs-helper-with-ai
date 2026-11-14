PRAGMA user_version = 2;
CREATE TABLE docs (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('flow','canvas')),
  name TEXT NOT NULL,
  seq INTEGER NOT NULL,
  hash TEXT NOT NULL,
  next_id INTEGER NOT NULL,
  origin_doc INTEGER, origin_seq INTEGER,
  created INTEGER NOT NULL, updated INTEGER NOT NULL,
  trashed INTEGER
);
INSERT INTO "docs" VALUES(1,'flow','이력서',11,'603d2a400c166b3d5da569189e598956e485db81e29c7366624a99614ea82ebc',6,NULL,NULL,1794657990824,1794657990947,NULL);
INSERT INTO "docs" VALUES(2,'canvas','포트폴리오',5,'8edab238a71f4b55f91bc288ef4a0d8d5051408e66aa164db2c5724cad69d4ac',5,NULL,NULL,1794657990954,1794657990996,NULL);
CREATE TABLE nodes (
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  id TEXT NOT NULL, parent TEXT, z TEXT NOT NULL, type TEXT NOT NULL,
  props TEXT NOT NULL, born INTEGER NOT NULL, rev INTEGER NOT NULL, revs TEXT NOT NULL,
  PRIMARY KEY (doc_id, id)
) WITHOUT ROWID;
INSERT INTO "nodes" VALUES(1,'n1','root','a0','para','{"runs":[{"t":"홍길동"}],"style":"title"}',0,2,'{"runs":1,"style":2}');
INSERT INTO "nodes" VALUES(1,'n2','root','a1','para','{"runs":[{"t":"견본상사 백엔드 개발"}]}',3,4,'{"runs":4}');
INSERT INTO "nodes" VALUES(1,'n3','root','a2','para','{"runs":[{"t":"결제 장애를 줄였다"}]}',4,4,'{}');
INSERT INTO "nodes" VALUES(1,'n4','root','a3','para','{"pf":{"list":{"kind":"ORDERED","level":0}},"runs":[{"t":"예시페이 연동"}]}',4,5,'{"pf":5}');
INSERT INTO "nodes" VALUES(1,'n5','root','a4','para','{"pf":{"list":{"kind":"ORDERED","level":0}},"runs":[{"t":""}]}',6,6,'{}');
INSERT INTO "nodes" VALUES(1,'root',NULL,'a0','flow','{}',0,0,'{}');
INSERT INTO "nodes" VALUES(1,'t7x1','root','a3V','table','{"cols":[240.95001220703125,240.95001220703125]}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x10','t7x7','a1','cell','{}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x11','t7x10','a0','para','{"runs":[{"t":""}]}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x2','t7x1','a0','row','{}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x3','t7x2','a0','cell','{}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x4','t7x3','a0','para','{"runs":[{"t":"기간"},{"o":{"asset":"2929f6ab46b95cf6d9b2d22340c459b55ee0c6f5c110607e4d025fc8107f17ca","h":22.5,"type":"image","w":30},"t":"￼"}]}',7,11,'{"runs":11}');
INSERT INTO "nodes" VALUES(1,'t7x5','t7x2','a1','cell','{}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x6','t7x5','a0','para','{"runs":[{"t":""}]}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x7','t7x1','a1','row','{}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x8','t7x7','a0','cell','{}',7,7,'{}');
INSERT INTO "nodes" VALUES(1,'t7x9','t7x8','a0','para','{"runs":[{"t":""}]}',7,7,'{}');
INSERT INTO "nodes" VALUES(2,'n1',NULL,'a0','page','{"h":1080,"name":"페이지 1","orientation":"landscape","preset":"SLIDE_16_9","w":1920}',0,0,'{}');
INSERT INTO "nodes" VALUES(2,'n2','n1','a0','rect','{"fills":[{"blend":"NORMAL","color":"#336699FF","opacity":1,"type":"SOLID","visible":true}],"h":100,"radius":8,"w":100,"x":100,"y":100}',1,2,'{"fills":2,"radius":2}');
INSERT INTO "nodes" VALUES(2,'n3','n1','a1','text','{"runs":[{"t":"프로젝트 소개"}],"x":100,"y":300}',3,4,'{"runs":4}');
INSERT INTO "nodes" VALUES(2,'n4','n1','a2','ellipse','{"h":100,"w":100,"x":400,"y":100}',5,5,'{}');
CREATE TABLE batches (
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  actor TEXT NOT NULL CHECK (actor IN ('user','ai')),
  kind TEXT NOT NULL CHECK (kind IN ('edit','undo','redo','restore')),
  label TEXT NOT NULL,
  grp INTEGER,
  unit INTEGER NOT NULL,
  run TEXT,
  target INTEGER,
  ops TEXT NOT NULL, inverse TEXT NOT NULL,
  hash TEXT NOT NULL,
  state INTEGER NOT NULL,
  format INTEGER NOT NULL DEFAULT 1,
  created INTEGER NOT NULL,
  PRIMARY KEY (doc_id, seq)
) WITHOUT ROWID;
INSERT INTO "batches" VALUES(1,1,'user','edit','입력',1,1,NULL,NULL,'[{"at":0,"del":0,"id":"n1","ins":[{"t":"홍길동"}],"op":"text","rev":1}]','[{"id":"n1","op":"set","props":{"runs":null}}]','859df9f5f6f1a4dd746bd3419201e64d2a28420926abe624cbe45e1a7540adec',0,1,1794657990863);
INSERT INTO "batches" VALUES(1,2,'user','edit','문단 모양',NULL,2,NULL,NULL,'[{"id":"n1","op":"set","props":{"style":"title"},"rev":2}]','[{"id":"n1","op":"set","props":{"style":null}}]','b60b9feb2b7a72e9c11498bed1a7cc1deebe6ab21c367177aec3048b8757a0cb',0,1,1794657990870);
INSERT INTO "batches" VALUES(1,3,'user','edit','문단 나누기',NULL,3,NULL,NULL,'[{"node":{"id":"n2","parent":"root","props":{"runs":[{"t":""}]},"rev":3,"type":"para","z":"a1"},"op":"add"}]','[{"id":"n2","op":"del"}]','cc5599b83124a35c3cd7c6fdde5bf0e40c7c89fe491bbd59a428f99cb340a4f2',0,1,1794657990876);
INSERT INTO "batches" VALUES(1,4,'user','edit','붙여넣기',NULL,4,NULL,NULL,'[{"at":0,"del":0,"id":"n2","ins":[{"t":"견본상사 백엔드 개발"}],"op":"text","rev":4},{"node":{"id":"n3","parent":"root","props":{"runs":[{"t":"결제 장애를 줄였다"}]},"rev":4,"type":"para","z":"a2"},"op":"add"},{"node":{"id":"n4","parent":"root","props":{"runs":[{"t":"예시페이 연동"}]},"rev":4,"type":"para","z":"a3"},"op":"add"}]','[{"id":"n4","op":"del"},{"id":"n3","op":"del"},{"at":0,"del":29,"id":"n2","ins":[{"t":""}],"op":"text"}]','be4d8bc32f7797143a351f048bb30df1c1c897667387f53f9480b8ebeea0df0f',0,1,1794657990881);
INSERT INTO "batches" VALUES(1,5,'user','edit','목록',NULL,5,NULL,NULL,'[{"id":"n4","op":"set","props":{"pf":{"list":{"kind":"ORDERED","level":0}}},"rev":5}]','[{"id":"n4","op":"set","props":{"pf":null}}]','cf71740201e7f5a3845538224e91daf17b416b44e8ad78ba449a928f7094d3dc',0,1,1794657990887);
INSERT INTO "batches" VALUES(1,6,'user','edit','문단 나누기',NULL,6,NULL,NULL,'[{"node":{"id":"n5","parent":"root","props":{"pf":{"list":{"kind":"ORDERED","level":0}},"runs":[{"t":""}]},"rev":6,"type":"para","z":"a4"},"op":"add"}]','[{"id":"n5","op":"del"}]','aadcc916b853e678a63bd33f7767e121c5a6bd77079d62047da96a768f0c5d00',0,1,1794657990897);
INSERT INTO "batches" VALUES(1,7,'user','edit','표',NULL,7,NULL,NULL,'[{"node":{"id":"t7x1","parent":"root","props":{"cols":[240.95001220703125,240.95001220703125]},"rev":7,"type":"table","z":"a3V"},"op":"add"},{"node":{"id":"t7x2","parent":"t7x1","props":{},"rev":7,"type":"row","z":"a0"},"op":"add"},{"node":{"id":"t7x3","parent":"t7x2","props":{},"rev":7,"type":"cell","z":"a0"},"op":"add"},{"node":{"id":"t7x4","parent":"t7x3","props":{"runs":[{"t":""}]},"rev":7,"type":"para","z":"a0"},"op":"add"},{"node":{"id":"t7x5","parent":"t7x2","props":{},"rev":7,"type":"cell","z":"a1"},"op":"add"},{"node":{"id":"t7x6","parent":"t7x5","props":{"runs":[{"t":""}]},"rev":7,"type":"para","z":"a0"},"op":"add"},{"node":{"id":"t7x7","parent":"t7x1","props":{},"rev":7,"type":"row","z":"a1"},"op":"add"},{"node":{"id":"t7x8","parent":"t7x7","props":{},"rev":7,"type":"cell","z":"a0"},"op":"add"},{"node":{"id":"t7x9","parent":"t7x8","props":{"runs":[{"t":""}]},"rev":7,"type":"para","z":"a0"},"op":"add"},{"node":{"id":"t7x10","parent":"t7x7","props":{},"rev":7,"type":"cell","z":"a1"},"op":"add"},{"node":{"id":"t7x11","parent":"t7x10","props":{"runs":[{"t":""}]},"rev":7,"type":"para","z":"a0"},"op":"add"}]','[{"id":"t7x11","op":"del"},{"id":"t7x10","op":"del"},{"id":"t7x9","op":"del"},{"id":"t7x8","op":"del"},{"id":"t7x7","op":"del"},{"id":"t7x6","op":"del"},{"id":"t7x5","op":"del"},{"id":"t7x4","op":"del"},{"id":"t7x3","op":"del"},{"id":"t7x2","op":"del"},{"id":"t7x1","op":"del"}]','9e0eff405a2d3784c8b0aab653dbe866226fc1700d99211589c4cb4334cf118d',0,1,1794657990904);
INSERT INTO "batches" VALUES(1,8,'user','edit','입력',2,8,NULL,NULL,'[{"at":0,"del":0,"id":"t7x4","ins":[{"t":"기간"}],"op":"text","rev":8}]','[{"at":0,"del":6,"id":"t7x4","ins":[{"t":""}],"op":"text"}]','9ad97864a787babf910a0b7342455e9d2a08329b6eb38fb67600574b85c05eed',0,1,1794657990909);
INSERT INTO "batches" VALUES(1,9,'user','edit','이미지',NULL,9,NULL,NULL,'[{"at":6,"del":0,"id":"t7x4","ins":[{"o":{"asset":"2929f6ab46b95cf6d9b2d22340c459b55ee0c6f5c110607e4d025fc8107f17ca","h":22.5,"type":"image","w":30},"t":"￼"}],"op":"text","rev":9}]','[{"at":6,"del":3,"id":"t7x4","ins":[],"op":"text"}]','603d2a400c166b3d5da569189e598956e485db81e29c7366624a99614ea82ebc',0,1,1794657990933);
INSERT INTO "batches" VALUES(1,10,'user','edit','입력',3,10,NULL,NULL,'[{"at":9,"del":0,"id":"t7x4","ins":[{"t":"지울 글"}],"op":"text","rev":10}]','[{"at":9,"del":10,"id":"t7x4","ins":[],"op":"text"}]','ca940dbb611585d8f02c27b1841f4ec2d8c5bf58348db1981932a580512aa574',1,1,1794657990941);
INSERT INTO "batches" VALUES(1,11,'user','undo','되돌리기: 입력',NULL,11,NULL,10,'[{"at":9,"del":10,"id":"t7x4","ins":[],"op":"text","rev":11}]','[{"at":9,"del":0,"id":"t7x4","ins":[{"t":"지울 글"}],"op":"text"}]','603d2a400c166b3d5da569189e598956e485db81e29c7366624a99614ea82ebc',2,1,1794657990947);
INSERT INTO "batches" VALUES(2,1,'user','edit','사각형 만들기',NULL,1,NULL,NULL,'[{"node":{"id":"n2","parent":"n1","props":{"h":100,"w":100,"x":100,"y":100},"rev":1,"type":"rect","z":"a0"},"op":"add"}]','[{"id":"n2","op":"del"}]','9415a90a254e399b3602a898c7d02313acf7ab8f177266944c79eb0e1f4b28ad',0,1,1794657990969);
INSERT INTO "batches" VALUES(2,2,'user','edit','속성 바꾸기',NULL,2,NULL,NULL,'[{"id":"n2","op":"set","props":{"fills":[{"blend":"NORMAL","color":"#336699FF","opacity":1,"type":"SOLID","visible":true}],"radius":8},"rev":2}]','[{"id":"n2","op":"set","props":{"fills":null,"radius":null}}]','016b4eb4b5942210222178c7576246f2f3dd0d0851b1d372490a0cd9d9d6735a',0,1,1794657990974);
INSERT INTO "batches" VALUES(2,3,'user','edit','글 상자 만들기',NULL,3,NULL,NULL,'[{"node":{"id":"n3","parent":"n1","props":{"x":100,"y":300},"rev":3,"type":"text","z":"a1"},"op":"add"}]','[{"id":"n3","op":"del"}]','b95a112b0fac9efc27b6a7dec8c730a93b3de2041166acee2b22a97babe1ce5f',0,1,1794657990985);
INSERT INTO "batches" VALUES(2,4,'user','edit','입력',1,4,NULL,NULL,'[{"at":0,"del":0,"id":"n3","ins":[{"t":"프로젝트 소개"}],"op":"text","rev":4}]','[{"id":"n3","op":"set","props":{"runs":null}}]','6f4a927f27b4e1817135683a96b67f91d34f18b6ecdfc40b8101b56e46ceeaec',0,1,1794657990991);
INSERT INTO "batches" VALUES(2,5,'user','edit','타원 만들기',NULL,5,NULL,NULL,'[{"node":{"id":"n4","parent":"n1","props":{"h":100,"w":100,"x":400,"y":100},"rev":5,"type":"ellipse","z":"a2"},"op":"add"}]','[{"id":"n4","op":"del"}]','8edab238a71f4b55f91bc288ef4a0d8d5051408e66aa164db2c5724cad69d4ac',0,1,1794657990996);
CREATE TABLE snapshots (
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL, state TEXT NOT NULL, hash TEXT NOT NULL,
  format INTEGER NOT NULL DEFAULT 1, created INTEGER NOT NULL,
  PRIMARY KEY (doc_id, seq)
) WITHOUT ROWID;
INSERT INTO "snapshots" VALUES(1,0,'{"format":1,"kind":"flow","name":"이력서","nextId":2,"nodes":[{"born":0,"id":"n1","parent":"root","props":{},"rev":0,"revs":{},"type":"para","z":"a0"},{"born":0,"id":"root","parent":null,"props":{},"rev":0,"revs":{},"type":"flow","z":"a0"}],"seq":0}','45e78b29278ad3894a0a2b867554782979aee52b86ed373b17738fbf7d6b019f',1,1794657990824);
INSERT INTO "snapshots" VALUES(2,0,'{"format":1,"kind":"canvas","name":"포트폴리오","nextId":2,"nodes":[{"born":0,"id":"n1","parent":null,"props":{"h":1080,"name":"페이지 1","orientation":"landscape","preset":"SLIDE_16_9","w":1920},"rev":0,"revs":{},"type":"page","z":"a0"}],"seq":0}','8c6b4156e957c8020cdca203369894a53891e32af3aacae2c7f3d021d128f47b',1,1794657990954);
CREATE TABLE versions (
  id INTEGER PRIMARY KEY,
  doc_id INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('auto','named')),
  reason TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL
);
INSERT INTO "versions" VALUES(1,1,5,'named','manual','첫 판',1794657990892);
INSERT INTO "versions" VALUES(2,2,2,'named','manual','첫 판',1794657990979);
INSERT INTO "versions" VALUES(3,1,11,'auto','close','',1794657991001);
INSERT INTO "versions" VALUES(4,2,5,'auto','close','',1794657991006);
CREATE TABLE assets (hash TEXT PRIMARY KEY, mime TEXT NOT NULL, w INTEGER, h INTEGER,
                     size INTEGER NOT NULL, created INTEGER NOT NULL) WITHOUT ROWID;
INSERT INTO "assets" VALUES('2929f6ab46b95cf6d9b2d22340c459b55ee0c6f5c110607e4d025fc8107f17ca','image/png',40,30,145,1794657990925);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE records (
  kind TEXT NOT NULL, id TEXT NOT NULL, updated INTEGER NOT NULL, body TEXT NOT NULL,
  PRIMARY KEY (kind, id)
) WITHOUT ROWID;
CREATE INDEX batches_undone ON batches(doc_id, actor, unit) WHERE state = 1;
CREATE INDEX versions_doc ON versions(doc_id, seq);
