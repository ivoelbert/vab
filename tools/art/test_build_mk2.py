"""Run: python3 -m unittest discover -s tools/art -p 'test_*.py'."""
import unittest
from PIL import Image
import build_mk2 as cabinet


class CabinetTests(unittest.TestCase):
    SIZE = (32, 48)

    @classmethod
    def setUpClass(cls):
        cls.views = [cabinet.render(cabinet.model(), cabinet.textures(), turns)
                     for turns in range(4)]

    def test_four_native_sprites_fit_without_clipping(self):
        for flat, _ in self.views:
            self.assertEqual(flat.size, self.SIZE)
            left, top, right, bottom = flat.getbbox()
            self.assertGreater(left, 0)
            self.assertGreater(top, 0)
            self.assertLess(right, self.SIZE[0])
            self.assertLess(bottom, self.SIZE[1])
            self.assertEqual(set(flat.getchannel('A').tobytes()), {0, 255})

    def test_layers_recompose_exactly(self):
        for flat, layers in self.views:
            self.assertEqual(tuple(layers), cabinet.LAYERS)
            composite = Image.new('RGBA', flat.size)
            for layer in layers.values():
                composite.alpha_composite(layer)
            self.assertEqual(composite.tobytes(), flat.tobytes())

    def test_front_only_details_are_not_painted_on_back(self):
        for turns, (_, layers) in enumerate(self.views):
            for name in ('screen', 'marquee', 'front'):
                self.assertEqual(layers[name].getbbox() is not None, turns < 2)
            self.assertIsNotNone(layers['side art'].getbbox())
            self.assertIsNotNone(layers['trim'].getbbox())

    def test_quarter_turns_preserve_the_cell_center(self):
        self.assertEqual(cabinet.rotate((8, 8, 0), 1, True), (8, 8, 0))
        self.assertEqual(cabinet.rotate((2, 13, 32), 4, True), (2, 13, 32))


if __name__ == '__main__':
    unittest.main()
