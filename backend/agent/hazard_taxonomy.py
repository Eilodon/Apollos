from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class HazardTaxonomyEntry:
    key: str
    description_vi: str
    aliases: tuple[str, ...]


HAZARD_TAXONOMY: tuple[HazardTaxonomyEntry, ...] = (
    HazardTaxonomyEntry(
        key='parked_motorbike',
        description_vi='Xe máy đậu chắn lối đi',
        aliases=(
            'motorbike',
            'xe may',
            'xe máy',
            'xe_hai_banh',
            'xe_hai_bánh',
            'parked_motorbike',
            'scooter',
            'motorcycle',
            'parked_scooter',
        ),
    ),
    HazardTaxonomyEntry(
        key='street_vendor',
        description_vi='Hàng quán hoặc xe đẩy lấn vỉa hè',
        aliases=('street_vendor', 'vendor', 'ban_hang', 'hang rong', 'xe day', 'via_he_lan_chiem'),
    ),
    HazardTaxonomyEntry(
        key='broken_pavement',
        description_vi='Vỉa hè hư hỏng hoặc mặt đường gồ ghề',
        aliases=('broken_pavement', 'vo_via_he', 'via_he_hong', 'uneven_surface', 'rough_surface', 'slippery_pavement'),
    ),
    HazardTaxonomyEntry(
        key='open_drain',
        description_vi='Cống hở hoặc hố ga không nắp',
        aliases=('open_drain', 'ho_ga', 'cong_ho', 'drain', 'manhole', 'hole', 'edge_drop'),
    ),
    HazardTaxonomyEntry(
        key='construction_barrier',
        description_vi='Rào chắn công trình hoặc vật cản thi công',
        aliases=('construction', 'construction_barrier', 'rao_chan', 'cong_trinh', 'barrier'),
    ),
    HazardTaxonomyEntry(
        key='overhead_obstacle',
        description_vi='Vật cản trên cao như biển hiệu hoặc nhánh cây thấp',
        aliases=('overhead_obstacle', 'bien_hieu_thap', 'nhanh_cay', 'low_signage', 'overhead', 'low_branch'),
    ),
)


def normalize_hazard_type(raw: str) -> str:
    normalized = str(raw or '').strip().lower().replace('-', '_')
    compact = '_'.join(normalized.split())
    if not compact:
        return 'unknown'
    for entry in HAZARD_TAXONOMY:
        if compact == entry.key or normalized == entry.key:
            return entry.key
        if compact in entry.aliases or normalized in entry.aliases:
            return entry.key
    return compact


def description_vi_for_hazard(hazard_type: str) -> str:
    key = normalize_hazard_type(hazard_type)
    for entry in HAZARD_TAXONOMY:
        if entry.key == key:
            return entry.description_vi
    return 'Vật cản chưa phân loại'
