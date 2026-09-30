"""Validate the enlarged four-player Sunset Riders cabinet."""
import unittest
import test_build_mk2 as common
import build_sunsetriders as cabinet


class SunsetRidersTests(common.CabinetTests):
    SIZE = (cabinet.WIDTH, cabinet.HEIGHT)

    @classmethod
    def setUpClass(cls):
        tex, solids = cabinet.textures(), cabinet.model()
        cls.views = [cabinet.base.render(solids, tex, turns, painter=cabinet.paint,
                                        width=cabinet.WIDTH, height=cabinet.HEIGHT)
                     for turns in range(4)]

    def test_four_distinct_control_stations(self):
        solids = cabinet.model()
        balls = [solid for solid in solids if isinstance(solid, cabinet.base.Ball)]
        buttons = [solid for solid in solids if solid.part == 'button'
                   and isinstance(solid, cabinet.base.Solid)]
        self.assertEqual(len(balls), 4)
        self.assertEqual(sum(solid.part == 'stick' for solid in solids), 4)
        self.assertEqual(len(buttons), 8)
        self.assertEqual(len({solid.color for solid in balls}), 4)
        self.assertEqual(len({solid.center[1] for solid in balls}), 4)
        for _, color in cabinet.PLAYERS:
            self.assertEqual(sum(solid.color == color for solid in buttons), 2)

    def test_deck_and_body_are_wider_than_two_player_shell(self):
        def width(solids, part):
            solid = next(solid for solid in solids if solid.part == part)
            # +y and -y half-space offsets give the model's width.
            return solid.planes[2][1] + solid.planes[3][1]
        self.assertEqual(width(cabinet.model(), 'deck'), 20)
        self.assertEqual(width(cabinet.shell.model(), 'deck'), 10)
        self.assertGreater(width(cabinet.model(), 'lower'), width(cabinet.shell.model(), 'lower'))

    def test_does_not_reuse_snowbros_art_or_gameplay(self):
        snow = cabinet.shell.textures()
        sunset = cabinet.textures()
        for name in ('left', 'right', 'marquee', 'bezel', 'controls', 'panel', 'screen'):
            self.assertNotEqual(sunset[name].tobytes(), snow[name].tobytes(), name)
        self.assertIn('front', sunset)

    def test_export_dimensions_match_editable_layers(self):
        from PIL import Image
        for facing in cabinet.base.FACINGS:
            name = f'cabinet_sunsetriders_{facing}'
            path = cabinet.ROOT / 'art/objects' / name
            text = (path / 'layers.ron').read_text()
            self.assertIn('width: 48', text)
            self.assertIn('height: 56', text)
            for index in range(len(cabinet.base.LAYERS)):
                with Image.open(path / f'{index}.png') as layer:
                    self.assertEqual(layer.size, self.SIZE)
            with Image.open(cabinet.ROOT / f'assets/tiles/objects/{name}.png') as flat:
                self.assertEqual(flat.size, self.SIZE)


if __name__ == '__main__':
    unittest.main()
