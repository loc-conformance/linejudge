document.querySelectorAll('.chip.pick').forEach(chip => {
  chip.addEventListener('click', () => {
    const group = chip.dataset.group;
    document.querySelectorAll('.dv[data-group="' + group + '"]').forEach(shown => {
      shown.hidden = shown.dataset.value !== chip.dataset.value;
    });
    document.querySelectorAll('.chip.pick[data-group="' + group + '"]').forEach(one => {
      one.classList.toggle('active', one === chip);
    });
  });
});

// Ticking a tool off hides its column and shrinks the table by exactly that column's width, so
// what is left closes up instead of leaving a gap to scroll past. What was ticked is remembered,
// because a reader who came to watch two tools wants them still chosen tomorrow.
const picks = document.querySelectorAll('.picks input[data-counter]');
if (picks.length) {
  const table = document.querySelector('table');
  const remembered = 'linejudge:hidden';
  let hidden = new Set();
  try {
    hidden = new Set(JSON.parse(localStorage.getItem(remembered) || '[]'));
  } catch (refused) {
    // storage is off, so every tool shows and nothing is remembered
  }
  const show = () => {
    document.querySelectorAll('[data-counter]').forEach(cell => {
      cell.classList.toggle('hidden', hidden.has(cell.dataset.counter) && !cell.matches('.picks *'));
    });
    table.style.setProperty('--shown', picks.length - hidden.size);
    try {
      localStorage.setItem(remembered, JSON.stringify([...hidden]));
    } catch (refused) {
      // the choice holds for this visit and is not carried to the next
    }
  };
  picks.forEach(box => {
    box.checked = !hidden.has(box.dataset.counter);
    box.addEventListener('change', () => {
      box.checked ? hidden.delete(box.dataset.counter) : hidden.add(box.dataset.counter);
      show();
    });
  });
  if (hidden.size) {
    show();
  }
}

// The file is read back out of the table it is painted in, so the line numbers and the buckets
// beside it are left behind and what lands on the clipboard is the file.
document.querySelectorAll('button.copy').forEach(button => {
  const said = button.querySelector('.said');
  button.addEventListener('click', () => {
    const lines = [...document.querySelectorAll('td.src')].map(cell => cell.textContent);
    navigator.clipboard.writeText(lines.join('\n') + '\n').then(() => {
      said.textContent = 'copied';
      setTimeout(() => { said.textContent = 'copy'; }, 1500);
    }, () => {
      said.textContent = 'select it by hand';
    });
  });
});

// Where this page was left, so that coming back to it lands where it was left. The back button
// alone would not be enough: the link out of a case is a plain forward navigation, and a browser
// restores nothing on one of those. A browser that refuses storage at all loses the memory and
// nothing else, which is why this is last and why it is wrapped.
try {
  const where = 'linejudge:' + location.pathname;
  const left = sessionStorage.getItem(where);
  if (left) {
    const [x, y] = left.split(',');
    scrollTo(Number(x), Number(y));
  }
  addEventListener('pagehide', () => {
    sessionStorage.setItem(where, scrollX + ',' + scrollY);
  });
} catch (refused) {
  // storage is off, so the page opens where a page opens
}
