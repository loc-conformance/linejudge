document.querySelectorAll('.chip.pick').forEach(chip => {
  chip.addEventListener('click', () => {
    const tool = chip.dataset.tool;
    document.querySelectorAll('.dv[data-tool="' + tool + '"]').forEach(shown => {
      shown.hidden = shown.dataset.d !== chip.dataset.d;
    });
    document.querySelectorAll('.chip.pick[data-tool="' + tool + '"]').forEach(one => {
      one.classList.toggle('active', one === chip);
    });
  });
});
