export function exportRows(result) {
  var out = [];
  for (var i = 0; i < result.rows.length; i++) {
    var r = result.rows[i];
    out.push(r.id + ',' + r.name + ',' + r.total);
  }
  return out;
}

export function summarize(rows) {
  var t = 0;
  for (var i = 0; i < rows.length; i++) t = t + rows[i].total;
  return { count: rows.length, total: t, average: t / rows.length };
}
