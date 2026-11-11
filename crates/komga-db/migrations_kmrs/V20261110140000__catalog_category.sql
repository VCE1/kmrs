-- Shelf taxonomy for this fork. A node may mirror one scan library by exact
-- name. That binding is the shelf the files already live on. It is not a
-- verdict that every book in the library was reviewed.
-- The previous per-library split (文学名作, 韩文小说, and the other 杂项
-- libraries) is intentionally not seeded.

CREATE TABLE CATALOG_CATEGORY (
    SLUG varchar PRIMARY KEY,
    PARENT_SLUG varchar,
    NAME varchar NOT NULL,
    SORT_ORDER integer NOT NULL DEFAULT 0,
    BOUND_LIBRARY_NAME varchar NOT NULL DEFAULT '',
    NOTE varchar NOT NULL DEFAULT '',
    -- 1: this shelf's children are the bound library's own folders.
    DIRECTORY_GROUPS integer NOT NULL DEFAULT 0,
    FOREIGN KEY (PARENT_SLUG) REFERENCES CATALOG_CATEGORY (SLUG)
);

CREATE TABLE CATALOG_SERIES (
    SERIES_ID varchar PRIMARY KEY,
    CATEGORY_SLUG varchar NOT NULL,
    FOREIGN KEY (CATEGORY_SLUG) REFERENCES CATALOG_CATEGORY (SLUG)
);

CREATE INDEX idx__catalog_category__parent ON CATALOG_CATEGORY (PARENT_SLUG, SORT_ORDER);
CREATE INDEX idx__catalog_series__category ON CATALOG_SERIES (CATEGORY_SLUG);

INSERT INTO CATALOG_CATEGORY (SLUG, PARENT_SLUG, NAME, SORT_ORDER, BOUND_LIBRARY_NAME, NOTE, DIRECTORY_GROUPS) VALUES
('light-novel', NULL, '轻小说', 10, '', '主分类。下面两格是现有书库货架，不代表每一本都已复核。简体和繁体可以同时保留。', 0),
('light-novel-refined', 'light-novel', '精修日文轻小说', 10, '精排日轻', '对应扫描书库「精排日轻」。', 0),
('light-novel-ja', 'light-novel', '日文轻小说', 20, '日文轻小说', '对应扫描书库「日文轻小说」。', 0),
('manga', NULL, '漫画', 20, '漫画', '主分类。不因为文件名或封面像小说就改挂到轻小说。', 0),
('programthink', NULL, '编程随想', 30, '编程随想推荐书库', '独立分类，来源和轻小说、漫画不同，不放进杂项。打开后按来源目录再分，同名文件夹留在各自的上级下面。', 1),
('pending', NULL, '待审核', 40, '', '类型还不能确认的作品放这里。文学名作、韩文、生活、计算机、网络、画集这些现有书库不是这套分类。', 0),
('misc', NULL, '杂项', 50, '', '确认不是轻小说、也不是漫画之后才放入。不自动归入。', 0);
