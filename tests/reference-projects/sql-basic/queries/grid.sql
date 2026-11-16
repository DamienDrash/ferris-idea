-- Result grid observation (OBS-0005): joined rows, enough for paging, sorting and filtering.
select m.id, w.city, m.sku, i.name, m.delta, m.moved_at
from movement m
join warehouse w on w.id = m.warehouse_id
join item i on i.sku = m.sku
order by m.id;
