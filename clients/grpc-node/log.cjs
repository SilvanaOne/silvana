require('log-timestamp')(() => {
  const d = new Date();
  // HH:MM:SS
  const time = d.toTimeString().split(' ')[0];
  // .mmm
  const ms = String(d.getMilliseconds()).padStart(3, '0');
  return `[${time}.${ms}]`;
}); 